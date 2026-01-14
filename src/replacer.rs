use crate::cli::Preset;
use crate::config::{ConfigLoader, ReplaceConfig};
use crate::errors::Result;
use crate::patterns::PatternManager;
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::Regex;
use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;

/// Core engine for finding and replacing patterns in files.
///
/// A `Replacer` is configured with a set of patterns, replacements, and blocks
/// to ignore. It can then process files, applying the configured transformations.
pub struct Replacer {
    patterns: Vec<Regex>,
    replacements: Vec<Option<String>>,
    blocks: Vec<BlockPattern>,
}

/// A compiled regex pattern for an ignorable block of text.
pub struct BlockPattern {
    regex: Regex,
    #[allow(dead_code)]
    start: String,
    #[allow(dead_code)]
    end: String,
}

/// Options for processing a file.
pub struct ProcessOptions {
    /// If `true`, a `.bak` file will be created before modifying a file.
    pub create_backup: bool,
    /// If `true`, changes will be calculated but not written to disk.
    pub dry_run: bool,
}

/// The result of processing a single file.
pub struct ProcessResult {
    /// The total number of changes made (lines replaced or blocks removed).
    pub changes: usize,
    /// `true` if the file was modified.
    pub modified: bool,
}

/// Statistics from an `undo` operation.
pub struct UndoStats {
    /// The number of backup files found.
    pub found: usize,
    /// The number of files successfully restored from backups.
    pub restored: usize,
}

impl Replacer {
    /// Creates a new `Replacer` from a `ReplaceConfig`.
    ///
    /// This involves compiling all the regex patterns from the configuration.
    pub fn new(config: ReplaceConfig) -> Result<Self> {
        let mut replacements = config.replacements.clone();
        if replacements.len() < config.patterns.len() {
            replacements.resize(config.patterns.len(), None);
        }

        // Compile regex patterns
        let regex_patterns: Vec<Regex> = config
            .patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Compile block patterns
        let blocks: Vec<BlockPattern> = config
            .blocks
            .iter()
            .map(|b| {
                let pattern = format!(
                    r"(?s){}.*?{}",
                    regex::escape(&b.start),
                    regex::escape(&b.end)
                );
                Ok(BlockPattern {
                    regex: Regex::new(&pattern)?,
                    start: b.start.clone(),
                    end: b.end.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            patterns: regex_patterns,
            replacements,
            blocks,
        })
    }

    /// Processes a single file, applying all configured replacements.
    ///
    /// The process is as follows:
    /// 1. Ignorable blocks are removed from the content.
    /// 2. Each pattern is applied in order. If a pattern has a corresponding
    ///    replacement string, a find-and-replace is performed. If the replacement
    ///    is `None`, the line *after* a matching line is removed.
    /// 3. If any changes were made and `dry_run` is false, the new content is
    ///    written to the file atomically.
    pub fn process_file(&self, path: &Path, options: ProcessOptions) -> Result<ProcessResult> {
        // Read file
        let content = fs::read_to_string(path)?;
        let mut new_content = Cow::Borrowed(content.as_str());
        let mut total_changes = 0;

        // Remove blocks first
        for block in &self.blocks {
            let matches = block.regex.find_iter(new_content.as_ref()).count();
            if matches > 0 {
                total_changes += matches;
                new_content = Cow::Owned(block.regex.replace_all(new_content.as_ref(), "").into_owned());
            }
        }

        // Clean up excessive empty lines after block removal
        // TODO: This cleanup logic could be more robust. A more sophisticated
        // approach would be to parse the code into an AST and remove nodes,
        // which would preserve formatting more reliably. For now, this is a
        // simple heuristic.
        if total_changes > 0 {
            new_content = Cow::Owned(clean_empty_lines(new_content.as_ref()));
        }

        // Process patterns
        for (i, pattern) in self.patterns.iter().enumerate() {
            if let Some(ref replacement) = self.replacements[i] {
                // Replace pattern
                let matches = pattern.find_iter(new_content.as_ref()).count();
                if matches > 0 {
                    total_changes += matches;
                    new_content =
                        Cow::Owned(pattern.replace_all(new_content.as_ref(), replacement).into_owned());
                }
            } else {
                // Delete lines after pattern
                let mut new_lines = Vec::new();
                let mut skip_next = false;
                let mut removed = 0;

                for line in new_content.lines() {
                    if skip_next {
                        removed += 1;
                        skip_next = false;
                        continue;
                    }
                    new_lines.push(line);
                    // Check if line matches the pattern (convert pattern to string for simple matching)
                    if pattern.is_match(line) {
                        skip_next = true;
                    }
                }

                if removed > 0 {
                    total_changes += removed;
                    let mut joined = new_lines.join("\n");
                    if !joined.ends_with('\n') && content.ends_with('\n') {
                        joined.push('\n');
                    }
                    new_content = Cow::Owned(joined);
                }
            }
        }

        // Write if changed
        if total_changes > 0 && !options.dry_run {
            if options.create_backup {
                let backup_path = format!("{}.bak", path.display());
                fs::copy(path, &backup_path)?;
            }

            // Write atomically using tempfile
            if let Some(parent) = path.parent() {
                let mut temp_file = NamedTempFile::new_in(parent)?;
                temp_file.write_all(new_content.as_ref().as_bytes())?;

                // Preserve file permissions
                let perms = fs::metadata(path)?.permissions();
                fs::set_permissions(temp_file.path(), perms)?;

                temp_file.persist(path)?;
            } else {
                return Err(format!("Could not get parent directory for {}", path.display()).into());
            }
        }

        Ok(ProcessResult {
            changes: total_changes,
            modified: total_changes > 0,
        })
    }

    /// Scans a directory for `.bak` files and restores them.
    ///
    /// # Arguments
    ///
    /// * `dir` - The directory to scan for backup files.
    /// * `keep_backups` - If `false`, the `.bak` files will be deleted after being restored.
    pub fn undo(dir: &Path, keep_backups: bool) -> Result<UndoStats> {
        let mut found = 0;
        let mut restored = 0;

        for entry in WalkBuilder::new(dir).build() {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("bak") {
                found += 1;
                let original_path = path.with_extension("");
                if path.exists() {
                    fs::copy(path, &original_path)?;
                    if !keep_backups {
                        fs::remove_file(path)?;
                    }
                    restored += 1;
                    println!("Restored {}", original_path.display());
                }
            }
        }

        Ok(UndoStats { found, restored })
    }
}

/// The main entry point for the `replace` command.
///
/// This function orchestrates the entire replacement process:
/// 1. It loads the configuration from a preset, a file, or command-line arguments.
/// 2. It walks the target directory to find all files to be processed.
/// 3. It uses a Rayon thread pool to process the files in parallel.
/// 4. It collects and prints summary statistics.
pub fn run_replace(
    preset: Option<Preset>,
    config_file: Option<PathBuf>,
    pattern: Option<String>,
    replacement: Option<String>,
    dir: PathBuf,
    extensions: Vec<String>,
    exclude: Vec<String>,
    no_backup: bool,
    dry_run: bool,
    verbose: bool,
    workers: Option<usize>,
) -> Result<()> {
    // Load or create config
    let config = if let Some(preset_type) = preset {
        // Use built-in preset
        println!("Using preset: {preset_type:?}");
        PatternManager::load_preset(&preset_type)
    } else if let Some(cfg_path) = config_file {
        // Use config file
        let resolved_path = ConfigLoader::find_config(&cfg_path, &dir)?;
        println!("Using config file: {}", resolved_path.display());
        ConfigLoader::load_replace_config(&resolved_path)?
    } else if let Some(pat) = pattern {
        // Use single pattern/replacement
        ReplaceConfig {
            patterns: vec![pat],
            replacements: vec![replacement],
            blocks: vec![],
            extensions: if extensions.is_empty() {
                None
            } else {
                Some(extensions.clone())
            },
            exclude: if exclude.is_empty() {
                None
            } else {
                Some(exclude.clone())
            },
        }
    } else {
        return Err("Specify --preset, --config, or --pattern".into());
    };

    // Normalize extensions
    let exts: Vec<String> = config
        .extensions
        .as_ref()
        .map(|e| {
            e.iter()
                .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                .collect()
        })
        .unwrap_or_else(|| {
            extensions
                .iter()
                .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                .collect()
        });

    // Get exclude directories from config or command line
    let exclude_dirs = config.exclude.clone().unwrap_or_else(|| exclude.clone());

    // Create replacer
    let replacer = Arc::new(Replacer::new(config)?);

    // Collect all files
    let mut all_files = Vec::new();
    let mut walker = WalkBuilder::new(&dir);
    walker.standard_filters(true); // Respect .gitignore

    for entry in walker.build() {
        let entry = entry?;
        let path = entry.path();

        // Check if path should be excluded
        let should_exclude = exclude_dirs
            .iter()
            .any(|ex| path.components().any(|c| c.as_os_str() == ex.as_str()));

        if !should_exclude && path.is_file() && should_process_file(path, &exts) {
            all_files.push(path.to_path_buf());
        }
    }

    // Stats
    let processed = AtomicUsize::new(0);
    let modified = AtomicUsize::new(0);
    let total_changes = AtomicUsize::new(0);

    // Process files in parallel
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        }))
        .build()?;

    let options = ProcessOptions {
        create_backup: !no_backup,
        dry_run,
    };

    let log_changes = verbose || dry_run;

    pool.install(|| {
        all_files.par_iter().for_each(|path| {
            match replacer.process_file(path, options.clone()) {
                Ok(result) => {
                    processed.fetch_add(1, Ordering::Relaxed);
                    if result.modified {
                        modified.fetch_add(1, Ordering::Relaxed);
                        total_changes.fetch_add(result.changes, Ordering::Relaxed);
                        if log_changes {
                            if dry_run {
                                println!(
                                    "DRY Modified {} ({} changes)",
                                    path.display(),
                                    result.changes
                                );
                            } else {
                                println!("Modified {} ({} changes)", path.display(), result.changes);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error processing file {}: {}", path.display(), e);
                }
            }
        });
    });

    println!("\n{}", "-".repeat(50));
    println!("Files scanned : {}", processed.load(Ordering::Relaxed));
    println!("Files changed : {}", modified.load(Ordering::Relaxed));
    println!("Total edits   : {}", total_changes.load(Ordering::Relaxed));

    Ok(())
}

/// The main entry point for the `undo` command.
pub fn run_undo(dir: PathBuf, keep_backups: bool) -> Result<()> {
    let stats = Replacer::undo(&dir, keep_backups)?;
    println!(
        "\nBackups found: {}, restored: {}",
        stats.found, stats.restored
    );
    Ok(())
}

/// The main entry point for the `clean-backups` command.
pub fn run_clean_backups(dir: PathBuf, dry_run: bool) -> Result<()> {
    let mut found = 0;
    let mut removed = 0;
    let mut total_size = 0u64;

    println!("Searching for backup files in {}...\n", dir.display());

    for entry in WalkBuilder::new(&dir).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("bak") {
            found += 1;

            if let Ok(metadata) = path.metadata() {
                total_size += metadata.len();
            }

            if dry_run {
                println!("Would remove: {}", path.display());
            } else {
                match fs::remove_file(path) {
                    Ok(_) => {
                        removed += 1;
                        println!("Removed: {}", path.display());
                    }
                    Err(e) => {
                        eprintln!("Failed to remove {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    println!("\n{}", "-".repeat(50));
    if dry_run {
        println!("Backup files found: {found}");
        println!("Total size: {:.2} MB", total_size as f64 / 1_048_576.0);
        println!("\nRun without --dry-run to remove these files");
    } else {
        println!("Backup files found: {found}");
        println!("Backup files removed: {removed}");
        println!("Space freed: {:.2} MB", total_size as f64 / 1_048_576.0);
    }

    Ok(())
}

impl Clone for ProcessOptions {
    fn clone(&self) -> Self {
        Self {
            create_backup: self.create_backup,
            dry_run: self.dry_run,
        }
    }
}

/// Determines if a file should be processed based on its extension.
fn should_process_file(path: &Path, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }

    path.extension()
        .and_then(|os| os.to_str())
        .map(|s| extensions.contains(&s.to_lowercase()))
        .unwrap_or(false)
}

/// Cleans up excessive empty lines from a string.
///
/// This is a heuristic to improve formatting after blocks of code have been removed.
/// It collapses multiple empty lines and removes leading/trailing empty lines.
fn clean_empty_lines(content: &str) -> String {
    // First pass: collapse multiple consecutive empty lines to at most 2
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut consecutive_empty = 0;

    for line in lines.iter() {
        if line.trim().is_empty() {
            consecutive_empty += 1;
            // Keep up to 2 empty lines
            if consecutive_empty <= 2 {
                result.push(*line);
            }
        } else {
            // Reset counter and add the line
            consecutive_empty = 0;
            result.push(*line);
        }
    }

    // Second pass: handle beginning of file
    let mut start_idx = 0;
    for (i, line) in result.iter().enumerate() {
        if !line.trim().is_empty() {
            start_idx = i;
            break;
        }
    }

    // Remove all empty lines at the beginning
    let result: Vec<&str> = result[start_idx..].to_vec();

    // Third pass: ensure no more than one empty line between content
    let mut final_result = Vec::new();
    let mut last_was_empty = false;

    for line in result {
        if line.trim().is_empty() {
            if !last_was_empty {
                final_result.push(line);
                last_was_empty = true;
            }
        } else {
            final_result.push(line);
            last_was_empty = false;
        }
    }

    // Join with newlines and preserve the final newline if it existed
    let mut output = final_result.join("\n");
    if content.ends_with('\n') && !output.ends_with('\n') {
        output.push('\n');
    }

    output
}
