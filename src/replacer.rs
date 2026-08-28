use crate::cli::Preset;
use crate::config::{ConfigLoader, ReplaceConfig};
use crate::errors::{Error, Result};
use crate::patterns::load_preset;
use crate::util::{is_backup_file, require_valid_extensions, resolve_workers, should_process_file};
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
#[derive(Clone, Copy)]
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
    /// For dry runs only: a `- old` / `+ new` preview of the changed lines.
    pub preview: Option<String>,
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
                    preview: None,
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

        // In a dry run, capture what would change so the caller can show it.
        let preview = if options.dry_run && total_changes > 0 {
            Some(dry_run_preview(&content, new_content.as_ref()))
        } else {
            None
        };

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
            preview,
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

        // Backups created by `replace` can be gitignored (e.g. "*.bak"), so
        // walk with ignore-file processing disabled — but keep hidden
        // filtering on: `replace` only processes files visible under the
        // standard filters, so a backup it creates can never live behind a
        // hidden path. Descending into hidden directories would restore
        // user-owned .bak files (e.g. a pre-existing .config/settings.bak)
        // over live files, and would walk .git.
        let mut walker = WalkBuilder::new(dir);
        walker
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false);

        for entry in walker.build() {
            // A single unreadable entry must not kill the whole run.
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("Warning: skipping unreadable path: {e}");
                    continue;
                }
            };
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

/// Options for [`run_replace`], mirroring the flags of the `replace` subcommand.
pub struct ReplaceOptions {
    /// A built-in preset to use for replacement.
    pub preset: Option<Preset>,
    /// Path to a YAML configuration file with replacement rules.
    pub config_file: Option<PathBuf>,
    /// A single regex pattern to search for.
    pub pattern: Option<String>,
    /// The replacement string for `pattern`.
    pub replacement: Option<String>,
    /// The directory to process.
    pub dir: PathBuf,
    /// File extensions to include. Empty means all files.
    pub extensions: Vec<String>,
    /// Directory names to exclude.
    pub exclude: Vec<String>,
    /// Disable the creation of `.bak` backup files.
    pub no_backup: bool,
    /// Preview the changes without modifying any files.
    pub dry_run: bool,
    /// Print each modified file.
    pub verbose: bool,
    /// Worker thread count. `None` uses the number of logical CPU cores.
    pub workers: Option<usize>,
}

/// The main entry point for the `replace` command.
///
/// This function orchestrates the entire replacement process:
/// 1. It loads the configuration from a preset, a file, or command-line arguments.
/// 2. It walks the target directory to find all files to be processed.
/// 3. It uses a Rayon thread pool to process the files in parallel.
/// 4. It collects and prints summary statistics.
pub fn run_replace(options: ReplaceOptions) -> Result<()> {
    let ReplaceOptions {
        preset,
        config_file,
        pattern,
        replacement,
        dir,
        extensions,
        exclude,
        no_backup,
        dry_run,
        verbose,
        workers,
    } = options;

    // Load or create config
    let config = if let Some(preset_type) = preset {
        // Use built-in preset; echo the kebab-case name the CLI accepts, not
        // the Debug form (`RemoveTodos`), which clap rejects if pasted back.
        use clap::ValueEnum;
        let preset_name = preset_type
            .to_possible_value()
            .expect("no Preset variant is #[value(skip)]");
        println!("Using preset: {}", preset_name.get_name());
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
    let exts = require_valid_extensions(exts_source)?;

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
        // A single unreadable entry (e.g. a permission-denied subdirectory)
        // must not kill the whole run, matching scan's behavior.
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("Warning: skipping unreadable path: {e}");
                continue;
            }
        };
        let path = entry.path();

        // Check if path should be excluded
        let should_exclude = exclude_dirs
            .iter()
            .any(|ex| path.components().any(|c| c.as_os_str() == ex.as_str()));

        // Never process .bak backups: rewriting a backup's content (and
        // stacking a .bak.bak on top) would make undo restore content the
        // original file never held.
        if !should_exclude
            && path.is_file()
            && !is_backup_file(path)
            && should_process_file(path, &exts)
        {
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
            .for_each(|path| match replacer.process_file(path, options) {
                Ok(result) => {
                    processed.fetch_add(1, Ordering::Relaxed);
                    if result.modified {
                        modified.fetch_add(1, Ordering::Relaxed);
                        total_changes.fetch_add(result.changes, Ordering::Relaxed);
                        if log_changes {
                            if dry_run {
                                // Build the whole per-file block first and emit
                                // it with a single print so parallel workers
                                // cannot interleave their previews.
                                let mut block = format!(
                                    "DRY Modified {} ({} changes)\n",
                                    path.display(),
                                    result.changes
                                );
                                if let Some(preview) = &result.preview {
                                    block.push_str(preview);
                                }
                                print!("{block}");
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

    // Backups created by `replace` can be gitignored (e.g. "*.bak"), so walk
    // with ignore-file processing disabled — but keep hidden filtering on:
    // `replace` only processes files visible under the standard filters, so a
    // backup it creates can never live behind a hidden path. Descending into
    // hidden directories would delete user-owned .bak files that oober never
    // created, and would walk .git.
    let mut walker = WalkBuilder::new(&dir);
    walker
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false);

    for entry in walker.build() {
        // A single unreadable entry must not kill the whole run.
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("Warning: skipping unreadable path: {e}");
                continue;
            }
        };
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

/// Maximum number of `-`/`+` lines shown per file in a dry-run preview.
const PREVIEW_MAX_LINES: usize = 40;

/// How many lines ahead the preview diff looks to re-align after a change.
const PREVIEW_RESYNC_WINDOW: usize = 50;

/// Builds a line-level preview of what a dry run would change: changed lines
/// as `- old` / `+ new` pairs, deletions as `-` lines, insertions as `+`
/// lines.
///
/// This is a greedy line diff, not a minimal one: on a mismatch it prefers
/// whichever of "lines deleted" or "lines inserted" re-aligns the two sides
/// soonest (within [`PREVIEW_RESYNC_WINDOW`] lines) and otherwise reports a
/// changed pair, so an ambiguous whole-file transform may attribute a change
/// to a nearby identical line. Output is capped at [`PREVIEW_MAX_LINES`]
/// lines with a `... and N more` note; a change `lines()` cannot see (e.g.
/// only the trailing newline differs) falls back to a note.
fn dry_run_preview(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff: Vec<String> = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < old_lines.len() && j < new_lines.len() {
        if old_lines[i] == new_lines[j] {
            i += 1;
            j += 1;
            continue;
        }
        // Distance until the old side re-aligns with the current new line
        // (pure deletion) or the new side re-aligns with the current old line
        // (pure insertion). Both are at least 1 since the lines differ.
        let deleted = old_lines[i..]
            .iter()
            .take(PREVIEW_RESYNC_WINDOW)
            .position(|l| *l == new_lines[j]);
        let inserted = new_lines[j..]
            .iter()
            .take(PREVIEW_RESYNC_WINDOW)
            .position(|l| *l == old_lines[i]);
        match (deleted, inserted) {
            (Some(d), n) if n.is_none_or(|n| d <= n) => {
                for line in &old_lines[i..i + d] {
                    diff.push(format!("- {line}"));
                }
                i += d;
            }
            (_, Some(n)) => {
                for line in &new_lines[j..j + n] {
                    diff.push(format!("+ {line}"));
                }
                j += n;
            }
            // `(None, None)`; a `(Some, None)` with a failed guard cannot
            // occur, but the compiler cannot see through the guard.
            _ => {
                diff.push(format!("- {}", old_lines[i]));
                diff.push(format!("+ {}", new_lines[j]));
                i += 1;
                j += 1;
            }
        }
    }
    for line in &old_lines[i..] {
        diff.push(format!("- {line}"));
    }
    for line in &new_lines[j..] {
        diff.push(format!("+ {line}"));
    }

    let mut out = String::new();
    for line in diff.iter().take(PREVIEW_MAX_LINES) {
        out.push_str(line);
        out.push('\n');
    }
    if diff.len() > PREVIEW_MAX_LINES {
        out.push_str(&format!(
            "  ... and {} more\n",
            diff.len() - PREVIEW_MAX_LINES
        ));
    }
    if diff.is_empty() {
        out.push_str("  (changes not attributable to specific lines)\n");
    }
    out
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
    fn test_undo_and_clean_backups_find_gitignored_but_skip_hidden_backups() {
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

        // A gitignored backup, and a hidden file's .bak. `replace` never
        // processes hidden paths, so the latter is user-owned and must not
        // be restored.
        fs::write(dir.join("a.txt"), "modified").unwrap();
        fs::write(dir.join("a.txt.bak"), "original").unwrap();
        fs::write(dir.join(".env"), "live").unwrap();
        fs::write(dir.join(".env.bak"), "user data").unwrap();

        let stats = Replacer::undo(dir, false).unwrap();
        assert_eq!(stats.found, 1);
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "original");
        assert!(!dir.join("a.txt.bak").exists());
        assert_eq!(
            fs::read_to_string(dir.join(".env")).unwrap(),
            "live",
            "undo must not restore a hidden .bak oober cannot have created"
        );
        assert!(dir.join(".env.bak").exists());

        // clean-backups discovery: gitignored backups are removed, hidden
        // .bak files are left alone.
        fs::write(dir.join("b.txt.bak"), "stale").unwrap();
        run_clean_backups(dir.to_path_buf(), false).unwrap();
        assert!(!dir.join("b.txt.bak").exists());
        assert!(dir.join(".env.bak").exists());
    }

    #[test]
    fn test_undo_and_clean_backups_skip_user_owned_hidden_dir_backups() {
        // Verified failure: undo walked into hidden directories and copied a
        // pre-existing user-owned .config/settings.bak over the live
        // .config/settings. `replace` cannot create backups behind hidden
        // paths, so undo and clean-backups must never touch them.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::create_dir(dir.join(".config")).unwrap();
        let settings = dir.join(".config").join("settings");
        let settings_bak = dir.join(".config").join("settings.bak");
        fs::write(&settings, "live").unwrap();
        fs::write(&settings_bak, "old").unwrap();

        let stats = Replacer::undo(dir, false).unwrap();
        assert_eq!(stats.found, 0);
        assert_eq!(stats.restored, 0);
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "live",
            "undo must not overwrite live files from user-owned hidden backups"
        );
        assert!(settings_bak.exists());

        run_clean_backups(dir.to_path_buf(), false).unwrap();
        assert!(
            settings_bak.exists(),
            "clean-backups must not delete user-owned hidden backups"
        );
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

        let result = run_replace(ReplaceOptions {
            preset: None,
            config_file: Some(config_path),
            pattern: None,
            replacement: None,
            dir: dir.to_path_buf(),
            extensions: vec![],
            exclude: vec![],
            no_backup: false, // backups enabled, none should be created
            dry_run: false,
            verbose: false,
            workers: Some(1),
        });

        assert!(result.is_err(), "mismatched config must be a hard error");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            original,
            "no file may be touched when the config is rejected"
        );
        assert!(!dir.join("t.txt.bak").exists());
    }

    #[test]
    fn test_all_invalid_extensions_filter_is_rejected_and_touches_nothing() {
        // Verified failure: `-x .` normalized to an empty set, and an empty
        // set means "process every file" — so a filter meant to select
        // nothing rewrote extensionless and .py files alike.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.py"), "AAA\n").unwrap();
        fs::write(dir.join("noext"), "AAA\n").unwrap();

        let result = run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("AAA".into()),
            replacement: Some("BBB".into()),
            dir: dir.to_path_buf(),
            extensions: vec![".".to_string()],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        });

        let err = result.expect_err("-x . must be rejected");
        assert!(
            err.to_string().contains("no valid extensions"),
            "error must name the empty filter: {err}"
        );
        assert_eq!(fs::read_to_string(dir.join("a.py")).unwrap(), "AAA\n");
        assert_eq!(fs::read_to_string(dir.join("noext")).unwrap(), "AAA\n");
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

        run_replace(ReplaceOptions {
            preset: None,
            config_file: Some(config_path),
            pattern: None,
            replacement: None,
            dir: dir.to_path_buf(),
            extensions: vec!["py".to_string()], // explicit -x must win over the config's [js]
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
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

        run_replace(ReplaceOptions {
            preset: None,
            config_file: Some(config_path),
            pattern: None,
            replacement: None,
            dir: dir.to_path_buf(),
            extensions: vec!["txt".to_string()],
            // explicit --exclude must win over the config's [vendor]
            exclude: vec!["generated".to_string()],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
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
    fn test_dry_run_preview_pairs_changed_lines() {
        let preview = dry_run_preview("foo one\nkeep me\nfoo two\n", "bar one\nkeep me\nbar two\n");
        assert_eq!(preview, "- foo one\n+ bar one\n- foo two\n+ bar two\n");
    }

    #[test]
    fn test_dry_run_preview_attributes_deletions_and_insertions() {
        assert_eq!(dry_run_preview("a\nDELETED\nb\n", "a\nb\n"), "- DELETED\n");
        assert_eq!(
            dry_run_preview("a\nb\n", "a\nINSERTED\nb\n"),
            "+ INSERTED\n"
        );
        // Multi-line block removal attributes every removed line.
        assert_eq!(
            dry_run_preview("a\nX1\nX2\nX3\nb\n", "a\nb\n"),
            "- X1\n- X2\n- X3\n"
        );
    }

    #[test]
    fn test_dry_run_preview_caps_output_with_more_note() {
        let old: String = (0..30).map(|n| format!("old {n}\n")).collect();
        let new: String = (0..30).map(|n| format!("new {n}\n")).collect();
        let preview = dry_run_preview(&old, &new);
        // 30 changed lines make 60 diff lines: 40 shown, 20 elided.
        assert_eq!(preview.lines().count(), 41);
        assert!(
            preview.ends_with("  ... and 20 more\n"),
            "capped preview must count the elided lines: {preview}"
        );
    }

    #[test]
    fn test_dry_run_preview_notes_changes_lines_cannot_see() {
        // Only the trailing newline differs; there is no line to attribute.
        assert_eq!(
            dry_run_preview("a\n", "a"),
            "  (changes not attributable to specific lines)\n"
        );
    }

    #[test]
    fn test_dry_run_process_file_returns_preview_and_leaves_file_untouched() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        let original = "alpha one\nkeep me\n";
        fs::write(&file, original).unwrap();

        let replacer = single_pattern_replacer("alpha", "beta");
        let result = replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: true,
                    dry_run: true,
                },
            )
            .unwrap();

        assert!(result.modified);
        assert_eq!(
            result.preview.as_deref(),
            Some("- alpha one\n+ beta one\n"),
            "dry run must carry a line preview"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), original);
        assert!(!temp_dir.path().join("t.txt.bak").exists());

        // A real run does not pay for a preview.
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
        assert!(result.preview.is_none());
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

        let result = run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("alpha".into()),
            replacement: Some("beta".into()),
            dir: dir.to_path_buf(),
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        });

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

        let result = run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("alpha".into()),
            replacement: Some("beta".into()),
            dir: dir.to_path_buf(),
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        });

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

    #[cfg(unix)]
    #[test]
    fn test_replace_continues_past_unreadable_subdir() {
        use std::os::unix::fs::PermissionsExt;

        // Verified failure: one chmod-000 subdirectory aborted the walk via
        // `entry?` before any file was collected, so zero files were
        // processed — while scan on the same tree warned and continued.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("readable.txt"), "alpha one\n").unwrap();
        let locked = dir.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("unreachable.txt"), "alpha two\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let result = run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("alpha".into()),
            replacement: Some("beta".into()),
            dir: dir.to_path_buf(),
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        });

        // Restore permissions before asserting so TempDir cleanup succeeds.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            result.is_ok(),
            "an unreadable subdirectory must not fail the run: {:?}",
            result.err().map(|e| e.to_string())
        );
        assert_eq!(
            fs::read_to_string(dir.join("readable.txt")).unwrap(),
            "beta one\n",
            "the readable file must still be processed"
        );
        assert_eq!(
            fs::read_to_string(locked.join("unreachable.txt")).unwrap(),
            "alpha two\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_undo_and_clean_backups_continue_past_unreadable_subdir() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.txt"), "modified").unwrap();
        fs::write(dir.join("a.txt.bak"), "original").unwrap();
        let locked = dir.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let undo_result = Replacer::undo(dir, true);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let stats = undo_result.expect("an unreadable subdirectory must not fail undo");
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "original");

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let clean_result = run_clean_backups(dir.to_path_buf(), false);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        clean_result.expect("an unreadable subdirectory must not fail clean-backups");
        assert!(!dir.join("a.txt.bak").exists());
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
        assert!(first.process_file(&file, options).unwrap().modified);
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

    #[test]
    fn test_second_replace_run_skips_backup_files() {
        // Verified failure: run_replace's file collection picked up the .bak
        // files a previous run created, so a second run rewrote the backup's
        // content in place and stacked an a.txt.bak.bak on top — and undo
        // then "restored" content the file never contained.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("a.txt");
        fs::write(&file, "alpha one\n").unwrap();

        let run = |pattern: &str, replacement: &str| {
            run_replace(ReplaceOptions {
                preset: None,
                config_file: None,
                pattern: Some(pattern.into()),
                replacement: Some(replacement.into()),
                dir: dir.to_path_buf(),
                extensions: vec![],
                exclude: vec![],
                no_backup: false,
                dry_run: false,
                verbose: false,
                workers: Some(1),
            })
            .unwrap()
        };

        // First run: alpha -> beta, creating a.txt.bak with the original.
        run("alpha", "beta");
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");
        assert_eq!(
            fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
            "alpha one\n"
        );

        // Second run: alpha -> gamma. Only the backup still contains "alpha";
        // it must be skipped, not rewritten.
        run("alpha", "gamma");
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");
        assert_eq!(
            fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
            "alpha one\n",
            "a second replace run must not rewrite backup contents"
        );
        assert!(
            !dir.join("a.txt.bak.bak").exists(),
            "a second replace run must not create a backup of a backup"
        );

        // Undo restores the true original.
        let stats = Replacer::undo(dir, false).unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
    }
}
