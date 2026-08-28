use crate::cli::Preset;
use crate::config::{ConfigLoader, ReplaceConfig};
use crate::errors::{Error, Result};
use crate::patterns::load_preset;
use crate::util::{normalize_extensions, resolve_workers, should_process_file};
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::{Captures, Regex};
use std::borrow::Cow;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::NamedTempFile;

/// Core engine for finding and replacing patterns in files.
///
/// A `Replacer` is configured with a set of patterns, replacements, and blocks
/// to ignore. It can then process files, applying the configured transformations.
pub struct Replacer {
    patterns: Vec<Regex>,
    replacements: Vec<Option<String>>,
    blocks: Vec<Regex>,
}

/// Options for processing a file.
pub struct ProcessOptions {
    /// If `true`, a `.bak` file will be created before modifying a file.
    /// An existing `.bak` is never overwritten, so the oldest backup always
    /// preserves the pre-oober original.
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

/// A `regex::Replacer` that expands the replacement string exactly like the
/// built-in `&str` replacer while counting matches, so a replacement and its
/// change count come from a single regex pass.
struct CountingReplacer<'a> {
    replacement: &'a str,
    count: &'a mut usize,
}

impl regex::Replacer for CountingReplacer<'_> {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        *self.count += 1;
        caps.expand(self.replacement, dst);
    }
}

impl Replacer {
    /// Creates a new `Replacer` from a `ReplaceConfig`.
    ///
    /// This involves compiling all the regex patterns from the configuration.
    pub fn new(config: ReplaceConfig) -> Result<Self> {
        // A missing replacement must never silently become `null` ("delete the
        // line after the match") — require the lengths to line up exactly.
        if config.replacements.len() != config.patterns.len() {
            return Err(format!(
                "config has {} patterns but {} replacements; each pattern needs a replacement entry (use an explicit `null` to delete the line after a match)",
                config.patterns.len(),
                config.replacements.len()
            )
            .into());
        }
        let replacements = config.replacements.clone();

        // Compile regex patterns
        let regex_patterns: Vec<Regex> = config
            .patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Compile block patterns
        let blocks: Vec<Regex> = config
            .blocks
            .iter()
            .map(|b| {
                let pattern = format!(
                    r"(?s){}.*?{}",
                    regex::escape(&b.start),
                    regex::escape(&b.end)
                );
                Regex::new(&pattern)
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;

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
        // Skip binary files using the same heuristic as the scanner: a null
        // byte in the first 1KB. A skipped binary is not an error.
        {
            let mut sample = [0u8; 1024];
            let n = fs::File::open(path)?.read(&mut sample)?;
            if sample[..n].contains(&0) {
                return Ok(ProcessResult {
                    changes: 0,
                    modified: false,
                });
            }
        }

        // Read file
        let content = fs::read_to_string(path)?;
        let mut new_content = Cow::Borrowed(content.as_str());
        let mut total_changes = 0;

        // Remove blocks first, counting matches during the single replace pass
        for block in &self.blocks {
            let mut matches = 0;
            let replaced = block.replace_all(
                new_content.as_ref(),
                CountingReplacer {
                    replacement: "",
                    count: &mut matches,
                },
            );
            if matches > 0 {
                total_changes += matches;
                let replaced = replaced.into_owned();
                new_content = Cow::Owned(replaced);
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
                // Replace pattern, counting matches during the single replace pass
                let mut matches = 0;
                let replaced = pattern.replace_all(
                    new_content.as_ref(),
                    CountingReplacer {
                        replacement: replacement.as_str(),
                        count: &mut matches,
                    },
                );
                if matches > 0 {
                    total_changes += matches;
                    let replaced = replaced.into_owned();
                    new_content = Cow::Owned(replaced);
                }
            } else {
                // Delete lines after pattern
                // `lines()` strips `\r`, so detect the line ending up front and
                // rejoin with it to keep CRLF files CRLF.
                let line_ending = detect_line_ending(new_content.as_ref());
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
                    let mut joined = new_lines.join(line_ending);
                    if !joined.ends_with('\n') && content.ends_with('\n') {
                        joined.push_str(line_ending);
                    }
                    new_content = Cow::Owned(joined);
                }
            }
        }

        // Write if changed
        if total_changes > 0 && !options.dry_run {
            if options.create_backup {
                let backup_path = format!("{}.bak", path.display());
                // Never overwrite an existing backup: the oldest backup is the
                // pre-oober original, which undo must be able to restore.
                if !Path::new(&backup_path).exists() {
                    fs::copy(path, &backup_path)?;
                }
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
                return Err(
                    format!("Could not get parent directory for {}", path.display()).into(),
                );
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

        // Backups may themselves be gitignored (e.g. "*.bak") or belong to
        // hidden files, so walk without the standard filters.
        let mut walker = WalkBuilder::new(dir);
        walker.standard_filters(false);

        for entry in walker.build() {
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
// TODO: collapse these arguments into an options struct (pending refactor, audit A3).
#[allow(clippy::too_many_arguments)]
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
        load_preset(&preset_type)
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

    // Normalize extensions: explicit CLI flags win, config values apply otherwise
    let exts_source: &[String] = if !extensions.is_empty() {
        &extensions
    } else {
        config.extensions.as_deref().unwrap_or_default()
    };
    let exts = normalize_extensions(exts_source);

    // Exclude directories: explicit CLI flags win, config values apply otherwise
    let exclude_dirs = if !exclude.is_empty() {
        exclude.clone()
    } else {
        config.exclude.clone().unwrap_or_default()
    };

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
    let errors = AtomicUsize::new(0);

    // Process files in parallel
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(resolve_workers(workers))
        .build()?;

    let options = ProcessOptions {
        create_backup: !no_backup,
        dry_run,
    };

    let log_changes = verbose || dry_run;

    pool.install(|| {
        all_files
            .par_iter()
            .for_each(|path| match replacer.process_file(path, options.clone()) {
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
                                println!(
                                    "Modified {} ({} changes)",
                                    path.display(),
                                    result.changes
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error processing file {}: {}", path.display(), e);
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            });
    });

    println!("\n{}", "-".repeat(50));
    println!("Files scanned : {}", processed.load(Ordering::Relaxed));
    println!("Files changed : {}", modified.load(Ordering::Relaxed));
    println!("Total edits   : {}", total_changes.load(Ordering::Relaxed));

    let error_count = errors.load(Ordering::Relaxed);
    if error_count > 0 {
        println!("Errors        : {error_count}");
        return Err(Error::ReplaceFailed(error_count));
    }

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

    // Backups may themselves be gitignored (e.g. "*.bak") or belong to
    // hidden files, so walk without the standard filters.
    let mut walker = WalkBuilder::new(&dir);
    walker.standard_filters(false);

    for entry in walker.build() {
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

/// Returns the line ending used by `content`: `"\r\n"` if any CRLF ending is
/// present, `"\n"` otherwise. Used to rebuild content split with `lines()`
/// (which strips `\r`) without converting CRLF files to LF.
fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Cleans up excessive empty lines from a string.
///
/// This is a heuristic to improve formatting after blocks of code have been removed.
/// It collapses multiple empty lines and removes leading/trailing empty lines.
/// The content's line ending style (LF or CRLF) is preserved.
fn clean_empty_lines(content: &str) -> String {
    let line_ending = detect_line_ending(content);

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
    let mut output = final_result.join(line_ending);
    if content.ends_with('\n') && !output.ends_with('\n') {
        output.push_str(line_ending);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_undo_and_clean_backups_find_gitignored_and_hidden_backups() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        // Reproduce the real-world setup where the target repo gitignores
        // "*.bak": backup discovery must not depend on git filters.
        match std::process::Command::new("git")
            .arg("init")
            .current_dir(dir)
            .output()
        {
            Ok(out) if out.status.success() => {}
            _ => {
                eprintln!("git unavailable; skipping gitignored-backup test");
                return;
            }
        }
        fs::write(dir.join(".gitignore"), "*.bak\n").unwrap();

        // A gitignored backup and a hidden file's backup.
        fs::write(dir.join("a.txt"), "modified").unwrap();
        fs::write(dir.join("a.txt.bak"), "original").unwrap();
        fs::write(dir.join(".env"), "modified").unwrap();
        fs::write(dir.join(".env.bak"), "original").unwrap();

        let stats = Replacer::undo(dir, false).unwrap();
        assert_eq!(stats.found, 2);
        assert_eq!(stats.restored, 2);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "original");
        assert_eq!(fs::read_to_string(dir.join(".env")).unwrap(), "original");
        assert!(!dir.join("a.txt.bak").exists());
        assert!(!dir.join(".env.bak").exists());

        // clean-backups discovery must see gitignored/hidden backups too.
        fs::write(dir.join("b.txt.bak"), "stale").unwrap();
        fs::write(dir.join(".env2.bak"), "stale").unwrap();
        run_clean_backups(dir.to_path_buf(), false).unwrap();
        assert!(!dir.join("b.txt.bak").exists());
        assert!(!dir.join(".env2.bak").exists());
    }

    #[test]
    fn test_mismatched_replacements_length_is_config_error() {
        let err = match Replacer::new(ReplaceConfig {
            patterns: vec!["AAA".into(), "BBB".into()],
            replacements: vec![Some("XXX".into())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        }) {
            Ok(_) => panic!("mismatched config must be rejected"),
            Err(e) => e,
        };

        let msg = err.to_string();
        assert!(
            msg.contains("2 patterns"),
            "message must name pattern count: {msg}"
        );
        assert!(
            msg.contains("1 replacements"),
            "message must name replacement count: {msg}"
        );
        assert!(
            msg.contains("null"),
            "message must say null entries are explicit: {msg}"
        );
    }

    #[test]
    fn test_mismatched_config_leaves_files_untouched() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        let original = "line AAA\nkeep me\nline BBB\ndo not delete\n";
        fs::write(&file, original).unwrap();

        let config_path = dir.join("bad_config.yaml");
        fs::write(
            &config_path,
            "patterns:\n  - \"AAA\"\n  - \"BBB\"\nreplacements:\n  - \"XXX\"\n",
        )
        .unwrap();

        let result = run_replace(
            None,
            Some(config_path),
            None,
            None,
            dir.to_path_buf(),
            vec![],
            vec![],
            false, // no_backup: backups enabled, none should be created
            false, // dry_run
            false, // verbose
            Some(1),
        );

        assert!(result.is_err(), "mismatched config must be a hard error");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            original,
            "no file may be touched when the config is rejected"
        );
        assert!(!dir.join("t.txt.bak").exists());
    }

    #[test]
    fn test_cli_extensions_override_config_extensions() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.js"), "AAA\n").unwrap();
        fs::write(dir.join("b.py"), "AAA\n").unwrap();

        let config_path = dir.join("config.yaml");
        fs::write(
            &config_path,
            "patterns:\n  - \"AAA\"\nreplacements:\n  - \"BBB\"\nextensions: [js]\n",
        )
        .unwrap();

        run_replace(
            None,
            Some(config_path),
            None,
            None,
            dir.to_path_buf(),
            vec!["py".to_string()], // explicit -x must win over the config's [js]
            vec![],
            true,  // no_backup
            false, // dry_run
            false, // verbose
            Some(1),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("a.js")).unwrap(),
            "AAA\n",
            "config extensions must not override explicit CLI -x"
        );
        assert_eq!(
            fs::read_to_string(dir.join("b.py")).unwrap(),
            "BBB\n",
            "CLI-selected extension must be processed"
        );
    }

    #[test]
    fn test_cli_exclude_overrides_config_exclude() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::create_dir(dir.join("vendor")).unwrap();
        fs::create_dir(dir.join("generated")).unwrap();
        fs::write(dir.join("vendor").join("a.txt"), "AAA\n").unwrap();
        fs::write(dir.join("generated").join("b.txt"), "AAA\n").unwrap();

        let config_path = dir.join("config.yaml");
        fs::write(
            &config_path,
            "patterns:\n  - \"AAA\"\nreplacements:\n  - \"BBB\"\nexclude: [vendor]\n",
        )
        .unwrap();

        run_replace(
            None,
            Some(config_path),
            None,
            None,
            dir.to_path_buf(),
            vec!["txt".to_string()],
            vec!["generated".to_string()], // explicit --exclude must win over the config's [vendor]
            true,                          // no_backup
            false,                         // dry_run
            false,                         // verbose
            Some(1),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("vendor").join("a.txt")).unwrap(),
            "BBB\n",
            "config exclude must not apply when CLI --exclude is given"
        );
        assert_eq!(
            fs::read_to_string(dir.join("generated").join("b.txt")).unwrap(),
            "AAA\n",
            "CLI-excluded directory must be skipped"
        );
    }

    fn single_pattern_replacer(pattern: &str, replacement: &str) -> Replacer {
        Replacer::new(ReplaceConfig {
            patterns: vec![pattern.into()],
            replacements: vec![Some(replacement.into())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        })
        .unwrap()
    }

    #[test]
    fn test_null_replacement_line_deletion_preserves_crlf() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        fs::write(&file, "one\r\nMARKER\r\ndelete me\r\nfour\r\n").unwrap();

        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec!["MARKER".into()],
            replacements: vec![None],
            blocks: vec![],
            extensions: None,
            exclude: None,
        })
        .unwrap();

        let result = replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        assert_eq!(result.changes, 1);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "one\r\nMARKER\r\nfour\r\n",
            "remaining lines must stay CRLF-terminated after a null-replacement deletion"
        );
    }

    #[test]
    fn test_block_removal_and_cleanup_preserve_crlf() {
        use crate::config::Block;

        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        fs::write(
            &file,
            "keep one\r\n/* DEBUG START */\r\ndebug stuff\r\n/* DEBUG END */\r\nkeep two\r\n",
        )
        .unwrap();

        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec![],
            replacements: vec![],
            blocks: vec![Block {
                start: "/* DEBUG START */".into(),
                end: "/* DEBUG END */".into(),
            }],
            extensions: None,
            exclude: None,
        })
        .unwrap();

        let result = replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        assert!(result.modified);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "keep one\r\n\r\nkeep two\r\n",
            "block removal + empty-line cleanup must stay CRLF-terminated"
        );
    }

    #[test]
    fn test_single_pass_replace_counts_and_output_match_find_iter() {
        use crate::config::Block;

        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        let original = "foo1 foo22 keep\n/* DEBUG START */\nnoise\n/* DEBUG END */\nfoo333\n/* DEBUG START */ x /* DEBUG END */\nplain\n";
        fs::write(&file, original).unwrap();

        let pattern = r"foo(\d+)";
        let replacement = "bar$1";
        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec![pattern.into()],
            replacements: vec![Some(replacement.into())],
            blocks: vec![Block {
                start: "/* DEBUG START */".into(),
                end: "/* DEBUG END */".into(),
            }],
            extensions: None,
            exclude: None,
        })
        .unwrap();

        let result = replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        // Counts must match what two-pass find_iter().count() reported:
        // 2 block matches + 3 pattern matches.
        assert_eq!(result.changes, 5);
        assert!(result.modified);

        // Output must be byte-identical to the plain &str replace_all path,
        // including `$1` capture-group expansion.
        let re = Regex::new(pattern).unwrap();
        let expected = re.replace_all("foo1 foo22 keep\n\nfoo333\n\nplain\n", replacement);
        assert_eq!(fs::read_to_string(&file).unwrap(), expected);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "bar1 bar22 keep\n\nbar333\n\nplain\n"
        );
    }

    #[test]
    fn test_clean_empty_lines_preserves_crlf() {
        assert_eq!(
            clean_empty_lines("a\r\n\r\n\r\n\r\nb\r\n"),
            "a\r\n\r\nb\r\n"
        );
        // LF content stays LF.
        assert_eq!(clean_empty_lines("a\n\n\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn test_trim_whitespace_preset_strips_interior_trailing_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.rs");
        fs::write(&file, "fn main() {   \n\tlet x = 1;\t\t\n}\n").unwrap();

        let replacer = Replacer::new(load_preset(&Preset::TrimWhitespace)).unwrap();
        let result = replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        assert_eq!(
            result.changes, 2,
            "trailing whitespace on interior lines must be matched (anchors need (?m))"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "fn main() {\n\tlet x = 1;\n}\n"
        );
    }

    #[test]
    fn test_remove_empty_comments_preset_matches_interior_lines() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.js");
        fs::write(&file, "code line\n//\nmore code\n").unwrap();

        let replacer = Replacer::new(load_preset(&Preset::RemoveEmptyComments)).unwrap();
        let result = replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        assert_eq!(
            result.changes, 1,
            "an interior lone // line must be matched (anchors need (?m))"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "code line\n\nmore code\n"
        );
    }

    #[test]
    fn test_replace_silently_skips_binary_files() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("text.txt"), "alpha one\n").unwrap();
        // Null bytes (binary heuristic) plus invalid UTF-8 so a read_to_string
        // attempt would fail: the file must be skipped before it is read.
        let binary = dir.join("blob.bin");
        let binary_content = b"alpha\x00\xff\xfe\x00binary".to_vec();
        fs::write(&binary, &binary_content).unwrap();

        let result = run_replace(
            None,
            None,
            Some("alpha".into()),
            Some("beta".into()),
            dir.to_path_buf(),
            vec![],
            vec![],
            true,  // no_backup
            false, // dry_run
            false, // verbose
            Some(1),
        );

        assert!(
            result.is_ok(),
            "a binary file must not make replace fail: {:?}",
            result.err().map(|e| e.to_string())
        );
        assert_eq!(
            fs::read(&binary).unwrap(),
            binary_content,
            "binary file must be untouched"
        );
        // Text files are still processed normally.
        assert_eq!(
            fs::read_to_string(dir.join("text.txt")).unwrap(),
            "beta one\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_replace_exits_nonzero_when_a_file_fails() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("readable.txt"), "alpha one\n").unwrap();
        let locked = dir.join("locked.txt");
        fs::write(&locked, "alpha two\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let result = run_replace(
            None,
            None,
            Some("alpha".into()),
            Some("beta".into()),
            dir.to_path_buf(),
            vec![],
            vec![],
            true,  // no_backup
            false, // dry_run
            false, // verbose
            Some(1),
        );

        // Restore permissions before asserting so TempDir cleanup succeeds.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

        match result {
            Err(Error::ReplaceFailed(1)) => {}
            other => panic!(
                "a failed file must make run_replace return Err(ReplaceFailed(1)), got: {:?}",
                other.map(|_| ())
            ),
        }
        // Readable files are still processed despite the failure.
        assert_eq!(
            fs::read_to_string(dir.join("readable.txt")).unwrap(),
            "beta one\n"
        );
        assert_eq!(fs::read_to_string(&locked).unwrap(), "alpha two\n");
    }

    #[test]
    fn test_repeated_replace_preserves_original_backup() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        fs::write(&file, "alpha one\n").unwrap();

        let options = ProcessOptions {
            create_backup: true,
            dry_run: false,
        };

        // First pass: alpha -> beta.
        let first = single_pattern_replacer("alpha", "beta");
        assert!(first.process_file(&file, options.clone()).unwrap().modified);
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");

        // Second pass: beta -> gamma. The existing backup must be preserved.
        let second = single_pattern_replacer("beta", "gamma");
        assert!(second.process_file(&file, options).unwrap().modified);
        assert_eq!(fs::read_to_string(&file).unwrap(), "gamma one\n");

        assert_eq!(
            fs::read_to_string(dir.join("t.txt.bak")).unwrap(),
            "alpha one\n",
            "backup must keep the pre-oober original after repeated runs"
        );

        // Undo restores the original content.
        let stats = Replacer::undo(dir, false).unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
    }
}
