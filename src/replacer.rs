use crate::backup_records::{self, BackupRecord};
use crate::cli::Preset;
use crate::config::{ConfigLoader, ReplaceConfig};
use crate::errors::{Error, Result};
use crate::patterns::load_preset;
use crate::util::{
    is_backup_file, is_excluded, require_valid_extensions, resolve_workers, should_process_file,
    validate_replacement,
};
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::{Captures, Regex};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;
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
    /// An existing `.bak` is never overwritten: it is kept when it is oober's
    /// own recorded backup and the file is unchanged since oober last wrote
    /// it, so the oldest backup preserves the pre-oober original. Otherwise
    /// the file is refused.
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

/// Options for [`Replacer::undo`], mirroring the flags of the `undo` subcommand.
#[derive(Clone, Copy)]
pub struct UndoOptions {
    /// If `true`, the `.bak` files (and their records) are kept after restoring.
    pub keep_backups: bool,
    /// If `true`, `*.bak` files oober has no record of creating are restored too.
    pub all_bak: bool,
    /// If `true`, the files that would be restored are listed and nothing changes.
    pub dry_run: bool,
}

/// Statistics from an `undo` operation.
pub struct UndoStats {
    /// The number of backup files undo acted on (or would, in a dry run).
    pub found: usize,
    /// The number of files successfully restored from backups.
    pub restored: usize,
    /// The number of backups that could not be restored.
    pub failed: usize,
    /// The number of `*.bak` files left alone because oober has no record of
    /// creating them. Always 0 with `all_bak`.
    pub unrecorded: usize,
}

/// The backups under a directory, as `undo` and `clean-backups` see them.
#[derive(Default)]
struct BackupCandidates {
    /// Backups oober has a record of creating, as (backup, original) paths.
    recorded: Vec<(PathBuf, PathBuf)>,
    /// `*.bak` files found by the walk that oober has no record of creating.
    unrecorded: Vec<PathBuf>,
    /// Recorded backups whose file no longer exists.
    stale: Vec<PathBuf>,
}

/// Finds the backups under `dir`: the recorded ones, plus every other
/// `*.bak` file the walk reaches.
fn find_backups(dir: &Path) -> BackupCandidates {
    // Recorded backups under `dir`, by the path their record is keyed by.
    // The walk below takes out the ones it reaches, so none is listed twice.
    let mut records: HashMap<PathBuf, BackupRecord> = fs::canonicalize(dir)
        .map(|canonical| backup_records::list_under(&canonical))
        .unwrap_or_default()
        .into_iter()
        .map(|record| (record.backup.clone(), record))
        .collect();
    let mut backups = BackupCandidates::default();

    // Backups created by `replace` can be gitignored (e.g. "*.bak"), so walk
    // with ignore-file processing disabled, but keep hidden filtering on:
    // descending into hidden directories would offer user-owned .bak files
    // (e.g. a pre-existing .config/settings.bak) to --all-bak, and would
    // walk .git. The backup of a hidden file named as a `replace` input is
    // found through its record instead.
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
        if path.is_file() && is_backup_file(path) {
            // A record makes a `.bak` oober's only while the file is still
            // the one the record describes: once the backup is deleted by
            // hand, a file the user creates at its path is theirs. A file
            // that cannot be looked at stays recorded, and acting on it
            // reports the error.
            let recorded = backup_records::canonical_path(path)
                .ok()
                .and_then(|key| records.remove(&key))
                .is_some_and(|record| {
                    fs::symlink_metadata(path).map_or(true, |m| record.describes(&m))
                });
            if recorded {
                backups
                    .recorded
                    .push((path.to_path_buf(), path.with_extension("")));
            } else {
                backups.unrecorded.push(path.to_path_buf());
            }
        }
    }

    // Recorded backups the walk did not reach: backups of hidden inputs,
    // and paths where something other than a file sits now. Only a backup
    // that is known to be gone is stale; one that cannot be looked at (e.g.
    // inside an unreadable directory) stays recorded, and acting on it
    // reports the error. Whatever else sits at the path is not the backup
    // oober made, and the walk did not offer it to --all-bak either.
    for (backup, record) in records {
        match fs::symlink_metadata(&backup) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => backups.stale.push(backup),
            Ok(metadata) if !record.describes(&metadata) => eprintln!(
                "Warning: leaving {} alone: it is not the backup oober recorded there",
                backup.display()
            ),
            _ => backups.recorded.push((backup, record.original)),
        }
    }

    backups
}

/// Decides what `replace` does about the backup of `path` before changing
/// it. Returns the `.bak` path, plus the record of the backup already there
/// when that backup is kept; `None` means a new backup must be created.
///
/// An existing backup is kept only when it is oober's own (recorded, and
/// still the regular file its record describes) and the file's size and
/// mtime still equal the record's:
/// nothing touched the file since oober last wrote it, so the oldest backup
/// still leads back to the pre-oober original. Anything else at the path is
/// an error. Modifying the file anyway would leave its current state with no
/// backup, and a later undo would wipe every edit made since the first run.
/// The error for a directory or a symlink is its own variant: undo and
/// clean-backups cannot clear those the way they clear a regular file.
fn check_backup(path: &Path) -> Result<(PathBuf, Option<BackupRecord>)> {
    let mut backup_path = path.as_os_str().to_owned();
    backup_path.push(".bak");
    let backup_path = PathBuf::from(backup_path);

    let Ok(backup_metadata) = fs::symlink_metadata(&backup_path) else {
        return Ok((backup_path, None));
    };
    if !backup_metadata.is_file() {
        return Err(Error::BackupPathTaken(backup_path));
    }
    let record =
        backup_records::load(&backup_path).filter(|record| record.describes(&backup_metadata));
    if let Some(record) = record {
        let metadata = fs::metadata(path)?;
        if metadata.len() == record.size && metadata.modified()? == record.mtime {
            return Ok((backup_path, Some(record)));
        }
    }
    Err(Error::BackupExists(backup_path))
}

/// Writes `new_content` to `path` atomically. When `backup_file` is given,
/// the new, still empty backup is first filled with `content` (what `path`
/// holds now) and given the permissions of `path`.
fn write_file(
    path: &Path,
    backup_file: Option<fs::File>,
    content: &str,
    new_content: &str,
) -> Result<()> {
    if let Some(mut backup_file) = backup_file {
        backup_file.write_all(content.as_bytes())?;
        backup_file.set_permissions(fs::metadata(path)?.permissions())?;
    }

    // Write atomically using tempfile
    if let Some(parent) = path.parent() {
        let mut temp_file = NamedTempFile::new_in(parent)?;
        temp_file.write_all(new_content.as_bytes())?;

        // Preserve file permissions
        let perms = fs::metadata(path)?.permissions();
        fs::set_permissions(temp_file.path(), perms)?;

        temp_file.persist(path)?;
    } else {
        return Err(format!("Could not get parent directory for {}", path.display()).into());
    }
    Ok(())
}

/// Restores `original` from `backup`, writing through a temp file in the
/// original's directory and keeping the backup's permissions.
///
/// Refuses when the backup is not a regular file, or when something other
/// than a regular file sits at the original's path: neither is followed or
/// replaced.
fn restore_backup(backup: &Path, original: &Path) -> Result<()> {
    let backup_metadata = fs::symlink_metadata(backup)?;
    if !backup_metadata.is_file() {
        return Err(
            std::io::Error::other(format!("{} is not a regular file", backup.display())).into(),
        );
    }
    if fs::symlink_metadata(original).is_ok_and(|m| !m.is_file()) {
        return Err(std::io::Error::other(format!(
            "{} exists and is not a regular file",
            original.display()
        ))
        .into());
    }
    let Some(parent) = original.parent() else {
        return Err(format!("Could not get parent directory for {}", original.display()).into());
    };

    let mut temp_file = NamedTempFile::new_in(parent)?;
    std::io::copy(&mut fs::File::open(backup)?, &mut temp_file)?;
    fs::set_permissions(temp_file.path(), backup_metadata.permissions())?;
    temp_file.persist(original)?;
    Ok(())
}

/// Says how many `*.bak` files were left alone for lack of a backup record.
fn print_unrecorded_hint(unrecorded: usize) {
    if unrecorded > 0 {
        println!(
            "Left alone: {unrecorded} .bak file(s) oober has no record of creating (preview them with --all-bak --dry-run)"
        );
    }
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

        // A reference to a group the pattern does not define expands to
        // nothing; reject it before any file is touched.
        for (regex, replacement) in regex_patterns.iter().zip(&replacements) {
            if let Some(replacement) = replacement {
                validate_replacement(regex, replacement)?;
            }
        }

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

        // Remove blocks first
        for block in &self.blocks {
            let (replaced, matches) = remove_blocks(new_content.as_ref(), block);
            if matches > 0 {
                total_changes += matches;
                new_content = Cow::Owned(replaced);
            }
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
                // Rebuild from `split_inclusive` segments so every kept line
                // keeps its own terminator (LF or CRLF) byte-for-byte — files
                // can mix endings, so a single detected ending would rewrite
                // lines it never touched. Terminators are stripped only for
                // matching, mirroring `lines()`.
                let mut kept = String::with_capacity(new_content.len());
                let mut skip_next = false;
                let mut removed = 0;

                for segment in new_content.split_inclusive('\n') {
                    if skip_next {
                        removed += 1;
                        skip_next = false;
                        continue;
                    }
                    kept.push_str(segment);
                    let line = segment.strip_suffix('\n').unwrap_or(segment);
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    if pattern.is_match(line) {
                        skip_next = true;
                    }
                }

                if removed > 0 {
                    total_changes += removed;
                    new_content = Cow::Owned(kept);
                }
            }
        }

        // Settle the backup before anything is written. A dry run makes the
        // same check, so it reports the files a real run would refuse.
        let backup = if total_changes > 0 && options.create_backup {
            Some(check_backup(path)?)
        } else {
            None
        };

        // In a dry run, capture what would change so the caller can show it.
        let preview = if options.dry_run && total_changes > 0 {
            Some(dry_run_preview(&content, new_content.as_ref()))
        } else {
            None
        };

        // Write if changed
        if total_changes > 0 && !options.dry_run {
            // Nothing sits at the backup path: create the backup there.
            // `create_new` never follows a symlink and never overwrites a
            // file that appeared since the check.
            let new_backup = match &backup {
                Some((backup_path, None)) => Some(backup_path),
                _ => None,
            };
            let backup_file = new_backup
                .map(|backup_path| {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(backup_path)
                })
                .transpose()?;

            if let Err(e) = write_file(path, backup_file, &content, new_content.as_ref()) {
                // The file is as it was. Take back the backup this call
                // created: with no record, it would make the next run refuse
                // the file.
                if let Some(backup_path) = new_backup {
                    let _ = fs::remove_file(backup_path);
                }
                return Err(e);
            }

            // Record the backup as oober's own, with the size and mtime the
            // file has now that it is written: undo and clean-backups act
            // only on recorded backups, and the next replace keeps this
            // backup only while the file still matches. A kept older backup
            // has its record refreshed.
            if let Some((backup_path, record)) = backup {
                let created = record.map_or_else(SystemTime::now, |record| record.created);
                backup_records::write(&backup_path, path, created)
                    .map_err(|e| Error::BackupRecord(backup_path, e.to_string()))?;
            }
        }

        Ok(ProcessResult {
            changes: total_changes,
            modified: total_changes > 0,
            preview,
        })
    }

    /// Restores the files under `dir` from their `.bak` backups.
    ///
    /// Only backups oober has a record of creating are restored, unless
    /// `options.all_bak` also selects the other `*.bak` files in the tree.
    /// A restore that fails is reported and counted, and the rest still run.
    pub fn undo(dir: &Path, options: UndoOptions) -> Result<UndoStats> {
        let backups = find_backups(dir);
        let mut stats = UndoStats {
            found: 0,
            restored: 0,
            failed: 0,
            unrecorded: 0,
        };

        let mut targets = backups.recorded;
        if options.all_bak {
            targets.extend(backups.unrecorded.into_iter().map(|backup| {
                let original = backup.with_extension("");
                (backup, original)
            }));
        } else {
            stats.unrecorded = backups.unrecorded.len();
        }

        for (backup, original) in targets {
            stats.found += 1;
            if options.dry_run {
                println!("Would restore {}", original.display());
                continue;
            }

            let result = restore_backup(&backup, &original).and_then(|()| {
                if options.keep_backups {
                    // The record describes the file as oober last wrote it,
                    // and that is now the restore. A record that does not
                    // describe this backup (an unrecorded `.bak` restored
                    // with `all_bak`) is left as it is.
                    let metadata = fs::symlink_metadata(&backup)?;
                    let record =
                        backup_records::load(&backup).filter(|record| record.describes(&metadata));
                    if let Some(record) = record {
                        backup_records::write(&backup, &original, record.created)?;
                    }
                } else {
                    fs::remove_file(&backup)?;
                    backup_records::remove(&backup);
                }
                Ok(())
            });
            match result {
                Ok(()) => {
                    stats.restored += 1;
                    println!("Restored {}", original.display());
                }
                Err(e) => {
                    stats.failed += 1;
                    eprintln!("Failed to restore {}: {}", original.display(), e);
                }
            }
        }

        // A record whose backup is gone describes nothing any more.
        if !options.dry_run {
            for backup in &backups.stale {
                backup_records::remove_stale(backup);
            }
        }

        Ok(stats)
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
    /// The replacement string for `pattern`. Required when `pattern` is set.
    pub replacement: Option<String>,
    /// The input files and/or directories to process.
    pub inputs: Vec<PathBuf>,
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
/// 2. It collects the files to process from the inputs (walking directories,
///    taking explicit files as given), deduplicating files reached twice
///    and leaving out symlinks and the config file itself.
/// 3. It uses a Rayon thread pool to process the files in parallel.
/// 4. It collects and prints summary statistics.
pub fn run_replace(options: ReplaceOptions) -> Result<()> {
    let ReplaceOptions {
        preset,
        config_file,
        pattern,
        replacement,
        inputs,
        extensions,
        exclude,
        no_backup,
        dry_run,
        verbose,
        workers,
    } = options;

    // A lone pattern must never silently become a null replacement ("delete
    // the line after the match"). The CLI rejects this at parse time; this
    // guard covers library callers.
    if pattern.is_some() && replacement.is_none() {
        return Err(
            "a pattern needs a replacement (-r/--replacement); to delete the line after each match, use a YAML config with an explicit `null` replacement"
                .into(),
        );
    }

    // The config file in use, in canonical form; set below when one is given.
    let mut config_canonical: Option<PathBuf> = None;

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
        // Use config file. Resolve it relative to the first directory input
        // (or the first file's parent), so `-c name.yaml` keeps finding
        // configs that live in the processed tree, as it did with --dir.
        let config_dir = inputs
            .iter()
            .find(|p| p.is_dir())
            .cloned()
            .or_else(|| {
                inputs
                    .iter()
                    .find(|p| p.is_file())
                    .and_then(|f| f.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let resolved_path = ConfigLoader::find_config(&cfg_path, &config_dir)?;
        println!("Using config file: {}", resolved_path.display());
        config_canonical = fs::canonicalize(&resolved_path).ok();
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

    // Collect all files. A file reached twice (explicitly and via a directory
    // walk, or via overlapping directories) must be processed once: applying
    // the replacement twice would double-transform content and race the
    // parallel atomic writes. Paths are deduplicated by canonical form so
    // different spellings of one file collapse.
    //
    // The config file starts out as already seen, so it is never processed
    // when it lives inside the tree: rewriting it would change its own
    // patterns for the next run.
    let mut all_files: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = config_canonical.into_iter().collect();
    let mut add_file = |path: &Path| {
        let key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if seen.insert(key) {
            all_files.push(path.to_path_buf());
        }
    };

    for input in &inputs {
        if input.is_file() {
            // Explicit files pass the same filters scan applies to explicit
            // file inputs: the extension filter applies, and .bak backups are
            // never treated as input (binary files are skipped at processing
            // time, exactly as for walked files).
            if !is_backup_file(input) && should_process_file(input, &exts) {
                // Never process a symlink: the atomic write would replace the
                // link with a regular file and leave its target unchanged.
                if input
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
                {
                    eprintln!("Skipping symlink: {}", input.display());
                } else {
                    add_file(input);
                }
            }
        } else if input.is_dir() {
            let mut walker = WalkBuilder::new(input);
            walker.standard_filters(true); // Respect .gitignore

            // Excluded names are matched against the path below this input,
            // so the directories above it never count, and an excluded
            // directory is pruned instead of being walked and then filtered.
            let root = input.clone();
            let excludes = exclude_dirs.clone();
            walker.filter_entry(move |entry| {
                let path = entry.path();
                !is_excluded(path.strip_prefix(&root).unwrap_or(path), &excludes)
            });

            for entry in walker.build() {
                // A single unreadable entry (e.g. a permission-denied
                // subdirectory) must not kill the whole run, matching scan's
                // behavior.
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        eprintln!("Warning: skipping unreadable path: {e}");
                        continue;
                    }
                };
                let path = entry.path();

                // Never process .bak backups: rewriting a backup's content
                // (and stacking a .bak.bak on top) would make undo restore
                // content the original file never held.
                if path.is_file() && !is_backup_file(path) && should_process_file(path, &exts) {
                    // Symlinks are skipped here for the same reason as
                    // explicit symlink inputs above; a link's target inside
                    // the tree is still reached under its own path.
                    if entry.file_type().is_some_and(|t| t.is_symlink()) {
                        eprintln!("Skipping symlink: {}", path.display());
                    } else {
                        add_file(path);
                    }
                }
            }
        } else {
            return Err(format!("Input path not found: {}", input.display()).into());
        }
    }

    // A backup is recorded after its file has been written. Make sure the
    // records can be written before any file is changed: otherwise every
    // file is modified first and reported as an error afterwards.
    if !no_backup && !dry_run && !all_files.is_empty() {
        backup_records::check_writable()?;
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
pub fn run_undo(dir: PathBuf, options: UndoOptions) -> Result<()> {
    let stats = Replacer::undo(&dir, options)?;
    if options.dry_run {
        println!("\nBackups found: {}", stats.found);
        print_unrecorded_hint(stats.unrecorded);
        println!("\nRun without --dry-run to restore these files");
    } else {
        println!(
            "\nBackups found: {}, restored: {}",
            stats.found, stats.restored
        );
        print_unrecorded_hint(stats.unrecorded);
    }

    if stats.failed > 0 {
        println!("Errors: {}", stats.failed);
        return Err(Error::UndoFailed(stats.failed));
    }
    Ok(())
}

/// The main entry point for the `clean-backups` command.
///
/// Only backups oober has a record of creating are removed, unless `all_bak`
/// also selects the other `*.bak` files in the tree.
pub fn run_clean_backups(dir: PathBuf, dry_run: bool, all_bak: bool) -> Result<()> {
    let mut found = 0;
    let mut removed = 0;
    let mut total_size = 0u64;

    println!("Searching for backup files in {}...\n", dir.display());

    let backups = find_backups(&dir);
    let mut targets: Vec<PathBuf> = backups
        .recorded
        .into_iter()
        .map(|(backup, _)| backup)
        .collect();
    let unrecorded = if all_bak {
        targets.extend(backups.unrecorded);
        0
    } else {
        backups.unrecorded.len()
    };

    for path in &targets {
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
                    backup_records::remove(path);
                    println!("Removed: {}", path.display());
                }
                Err(e) => {
                    eprintln!("Failed to remove {}: {}", path.display(), e);
                }
            }
        }
    }

    // A record whose backup is gone describes nothing any more.
    if !dry_run {
        for backup in &backups.stale {
            backup_records::remove_stale(backup);
        }
    }

    println!("\n{}", "-".repeat(50));
    if dry_run {
        println!("Backup files found: {found}");
        println!("Total size: {:.2} MB", total_size as f64 / 1_048_576.0);
        print_unrecorded_hint(unrecorded);
        println!("\nRun without --dry-run to remove these files");
    } else {
        println!("Backup files found: {found}");
        println!("Backup files removed: {removed}");
        println!("Space freed: {:.2} MB", total_size as f64 / 1_048_576.0);
        print_unrecorded_hint(unrecorded);
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

/// Removes every match of `block` from `content`, returning the result and
/// the number of blocks removed.
///
/// A block that shares a line with other text is cut out of that line and
/// nothing else changes. A block that has its lines to itself goes with its
/// indent and its line break, and only the blank lines (spaces and tabs only)
/// directly next to it can go with it: the ones after it when it starts the
/// file or follows a blank line, and the ones before it when it ends the
/// file. Every other byte, line terminators included, is copied as is.
fn remove_blocks(content: &str, block: &Regex) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    let mut count = 0;

    // Empty start and end markers match the empty string, which removes
    // nothing and would never advance.
    while let Some(m) = block.find(rest).filter(|m| !m.is_empty()) {
        count += 1;
        out.push_str(&rest[..m.start()]);
        rest = &rest[m.end()..];

        // Judged against `out`, so a block is placed by what was kept in
        // front of it, not by a block removed just before it.
        let line_start = out.rfind('\n').map_or(0, |i| i + 1);
        if !is_blank(&out[line_start..]) {
            continue;
        }
        let Some(after_line) = strip_blank_line(rest).or_else(|| is_blank(rest).then_some(""))
        else {
            continue;
        };
        out.truncate(line_start);
        rest = after_line;

        if out.is_empty() || trailing_blank_line(&out).is_some() {
            while let Some(next) = strip_blank_line(rest) {
                rest = next;
            }
        }
        if rest.is_empty() {
            while let Some(start) = trailing_blank_line(&out) {
                out.truncate(start);
            }
        }
    }

    out.push_str(rest);
    (out, count)
}

/// Whether `text` holds nothing but spaces and tabs.
fn is_blank(text: &str) -> bool {
    text.bytes().all(|b| b == b' ' || b == b'\t')
}

/// `text` without its first line, when that line is blank and ends in a line
/// break (LF or CRLF).
fn strip_blank_line(text: &str) -> Option<&str> {
    let text = text.trim_start_matches([' ', '\t']);
    text.strip_prefix("\r\n")
        .or_else(|| text.strip_prefix('\n'))
}

/// The start of the last line of `text`, when that line is blank and ends in
/// a line break (LF or CRLF).
fn trailing_blank_line(text: &str) -> Option<usize> {
    let line = text.strip_suffix('\n')?;
    let line = line.strip_suffix('\r').unwrap_or(line);
    let start = line.rfind('\n').map_or(0, |i| i + 1);
    is_blank(&line[start..]).then_some(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// `undo` as run with no flags: only recorded backups are restored.
    const RECORDED_ONLY: UndoOptions = UndoOptions {
        keep_backups: false,
        all_bak: false,
        dry_run: false,
    };

    /// `undo --all-bak`, for tests that create their `.bak` files by hand.
    const ALL_BAK: UndoOptions = UndoOptions {
        all_bak: true,
        ..RECORDED_ONLY
    };

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

        let stats = Replacer::undo(dir, ALL_BAK).unwrap();
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
        run_clean_backups(dir.to_path_buf(), false, true).unwrap();
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

        let stats = Replacer::undo(dir, ALL_BAK).unwrap();
        assert_eq!(stats.found, 0);
        assert_eq!(stats.restored, 0);
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "live",
            "undo must not overwrite live files from user-owned hidden backups"
        );
        assert!(settings_bak.exists());

        run_clean_backups(dir.to_path_buf(), false, true).unwrap();
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
            inputs: vec![dir.to_path_buf()],
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
            inputs: vec![dir.to_path_buf()],
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
            inputs: vec![dir.to_path_buf()],
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
            inputs: vec![dir.to_path_buf()],
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

    #[test]
    fn test_exclude_matches_only_below_the_input_root() {
        // `ob replace <tmp>/build/app -e build`: the input sits under an
        // ancestor directory named like the exclude. Only a `build`
        // directory inside the input may be excluded.
        let temp_dir = TempDir::new().unwrap();
        let input = temp_dir.path().join("build").join("app");
        fs::create_dir_all(input.join("build")).unwrap();
        fs::write(input.join("a.txt"), "AAA\n").unwrap();
        fs::write(input.join("build").join("b.txt"), "AAA\n").unwrap();

        run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("AAA".into()),
            replacement: Some("BBB".into()),
            inputs: vec![input.clone()],
            extensions: vec![],
            exclude: vec!["build".to_string()],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(input.join("a.txt")).unwrap(),
            "BBB\n",
            "an ancestor of the input named like an exclude must not exclude the input"
        );
        assert_eq!(
            fs::read_to_string(input.join("build").join("b.txt")).unwrap(),
            "AAA\n",
            "an excluded directory inside the input must be skipped"
        );
    }

    #[test]
    fn test_explicit_file_input_is_processed_and_filtered() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.py"), "alpha\n").unwrap();
        fs::write(dir.join("b.py"), "alpha\n").unwrap();

        let run = |inputs: Vec<PathBuf>, extensions: Vec<String>| {
            run_replace(ReplaceOptions {
                preset: None,
                config_file: None,
                pattern: Some("alpha".into()),
                replacement: Some("beta".into()),
                inputs,
                extensions,
                exclude: vec![],
                no_backup: true,
                dry_run: false,
                verbose: false,
                workers: Some(1),
            })
        };

        // Only the explicitly named file is processed; its sibling is not.
        run(vec![dir.join("a.py")], vec![]).unwrap();
        assert_eq!(fs::read_to_string(dir.join("a.py")).unwrap(), "beta\n");
        assert_eq!(fs::read_to_string(dir.join("b.py")).unwrap(), "alpha\n");

        // The extension filter applies to explicit files, as in scan.
        run(vec![dir.join("b.py")], vec!["js".into()]).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("b.py")).unwrap(),
            "alpha\n",
            "-x must filter explicit file inputs too"
        );
    }

    #[test]
    fn test_explicit_bak_file_input_is_skipped() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.txt.bak"), "alpha\n").unwrap();

        run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("alpha".into()),
            replacement: Some("beta".into()),
            inputs: vec![dir.join("a.txt.bak")],
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
            "alpha\n",
            "an explicitly named .bak backup must never be rewritten"
        );
    }

    #[test]
    fn test_mixed_file_and_dir_inputs_process_each_file_once() {
        // The same file reached explicitly (under a different spelling) and
        // via a directory walk must be processed once: X -> XX applied twice
        // would leave XXXX behind.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.txt"), "X\n").unwrap();

        run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("X".into()),
            replacement: Some("XX".into()),
            inputs: vec![dir.join(".").join("a.txt"), dir.to_path_buf()],
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).unwrap(),
            "XX\n",
            "a deduplicated file must be transformed exactly once"
        );
    }

    #[test]
    fn test_pattern_without_replacement_is_rejected_and_touches_nothing() {
        // A lone pattern used to become a null replacement, which deletes
        // the line after each match.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.txt"), "alpha\nkeep me\n").unwrap();

        let err = run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("alpha".into()),
            replacement: None,
            inputs: vec![dir.to_path_buf()],
            extensions: vec![],
            exclude: vec![],
            no_backup: false,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
        .expect_err("a pattern without a replacement must be an error");
        assert!(
            matches!(err, Error::Config(_)) && err.to_string().contains("replacement"),
            "error must be a config error naming the replacement: {err}"
        );
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).unwrap(),
            "alpha\nkeep me\n"
        );
        assert!(!dir.join("a.txt.bak").exists());
    }

    #[test]
    fn test_nonexistent_input_path_errors_and_touches_nothing() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.txt"), "alpha\n").unwrap();

        let err = run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("alpha".into()),
            replacement: Some("beta".into()),
            inputs: vec![dir.to_path_buf(), dir.join("missing")],
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
        .expect_err("a missing input path must be an error");
        assert!(
            err.to_string().contains("Input path not found"),
            "error must name the missing path, like scan: {err}"
        );
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).unwrap(),
            "alpha\n",
            "no file may be modified when an input path is invalid"
        );
    }

    #[test]
    fn test_undefined_group_reference_in_replacement_is_config_error() {
        let config = |replacements: Vec<Option<String>>| ReplaceConfig {
            patterns: vec![r"(\d+)_(\w+)".into(), "PRICE".into()],
            replacements,
            blocks: vec![],
            extensions: None,
            exclude: None,
        };

        // `$2_` names a group called `2_`; `$5` an index past the last group.
        for bad in [
            vec![Some("$2_$1".to_string()), None],
            vec![Some("${2}_${1}".to_string()), Some("$5".to_string())],
        ] {
            let err = Replacer::new(config(bad))
                .err()
                .expect("an undefined group reference must be rejected");
            assert!(matches!(err, Error::Config(_)), "{err}");
        }

        // Defined references, a literal `$$`, and a null replacement pass.
        assert!(Replacer::new(config(vec![Some("${2}_${1}".to_string()), None])).is_ok());
        assert!(
            Replacer::new(config(vec![
                Some("$2".to_string()),
                Some("$$5".to_string())
            ]))
            .is_ok()
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
    fn test_null_replacement_line_deletion_preserves_mixed_line_endings() {
        // Verified failure: one CRLF line anywhere made detect_line_ending
        // declare the whole file CRLF, so the rebuild converted every LF
        // line to CRLF. Each kept line must keep its own ending
        // byte-for-byte; only the deleted line disappears.
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        fs::write(&file, "one\ntwo\r\nMARKER\ndelete me\nfive\n").unwrap();

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
            "one\ntwo\r\nMARKER\nfive\n",
            "mostly-LF file must not come back CRLF after a null-replacement deletion"
        );
    }

    #[test]
    fn test_block_removal_preserves_crlf() {
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
            "keep one\r\nkeep two\r\n",
            "block removal must stay CRLF-terminated"
        );
    }

    /// Runs a blocks-only replace over `content` and returns the file after it.
    fn remove_block(content: &str, start: &str, end: &str) -> String {
        use crate::config::Block;

        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        fs::write(&file, content).unwrap();

        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec![],
            replacements: vec![],
            blocks: vec![Block {
                start: start.into(),
                end: end.into(),
            }],
            extensions: None,
            exclude: None,
        })
        .unwrap();
        replacer
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        fs::read_to_string(&file).unwrap()
    }

    #[test]
    fn test_block_removal_leaves_distant_blank_lines_alone() {
        let header = "# Copyright (c) 2024 Example\n# Permission is hereby granted.\n# DEALINGS IN THE SOFTWARE.\n\n";
        // PEP 8 double blank lines between functions, and a triple-quoted
        // string that holds blank lines of its own.
        let body = "import os\n\n\ndef first():\n    return \"\"\"line one\n\n\nline four\n\"\"\"\n\n\ndef second():\n    return os.getcwd()\n";

        for eol in ["\n", "\r\n"] {
            let input = format!("{header}{body}").replace('\n', eol);
            assert_eq!(
                remove_block(&input, "# Copyright (c)", "# DEALINGS IN THE SOFTWARE."),
                body.replace('\n', eol),
                "only the header block and the blank line after it may go (eol {eol:?})"
            );
        }
    }

    #[test]
    fn test_block_removal_touches_only_adjacent_blank_lines() {
        let rm = |content: &str| remove_block(content, "# S", "# E");

        for eol in ["\n", "\r\n"] {
            let check = |input: &str, expected: &str| {
                assert_eq!(
                    rm(&input.replace('\n', eol)),
                    expected.replace('\n', eol),
                    "input {input:?} (eol {eol:?})"
                );
            };

            // Top of the file: no leading blank line is left.
            check("# S\nx\n# E\n\n\ncode\n", "code\n");
            // Between two paragraphs: the one separating blank line is left.
            check("one\n\n# S\nx\n# E\n\ntwo\n", "one\n\ntwo\n");
            // Between adjacent lines: no blank line appears.
            check("one\n# S\nx\n# E\ntwo\n", "one\ntwo\n");
            // Attached to the code above: the gap below it is kept whole.
            check(
                "def a():\n    y = 1\n    # S\n    x\n    # E\n\n\ndef b():\n",
                "def a():\n    y = 1\n\n\ndef b():\n",
            );
            // Attached to the code below: the gap above it is kept whole.
            check("one\n\n\n# S\nx\n# E\ntwo\n", "one\n\n\ntwo\n");
            // End of the file: no trailing blank line is left.
            check("code\n\n# S\nx\n# E\n", "code\n");
            check("code\n\n# S\nx\n# E", "code\n");
            // Consecutive blocks at the top are judged against what was kept.
            check("# S\nx\n# E\n\n# S\ny\n# E\n\ncode\n", "code\n");
            // Blank lines away from the block are not collapsed or stripped.
            check(
                "\n\none\n\n\n\ntwo\n# S\nx\n# E\nthree\n\n\n",
                "\n\none\n\n\n\ntwo\nthree\n\n\n",
            );
        }
    }

    #[test]
    fn test_block_removal_preserves_mixed_line_endings() {
        // Every kept line keeps its own terminator, and the blank lines that
        // go with the block are recognised whichever terminator they carry.
        assert_eq!(
            remove_block("one\r\n\n# S\r\nx\n# E\n\r\n\ntwo\r\nthree\n", "# S", "# E"),
            "one\r\n\ntwo\r\nthree\n"
        );
    }

    #[test]
    fn test_block_sharing_a_line_leaves_the_other_code_intact() {
        let rm = |content: &str| remove_block(content, "/* S */", "/* E */");

        for eol in ["\n", "\r\n"] {
            let check = |input: &str, expected: &str| {
                assert_eq!(
                    rm(&input.replace('\n', eol)),
                    expected.replace('\n', eol),
                    "input {input:?} (eol {eol:?})"
                );
            };

            // Code before the start marker keeps its line break.
            check(
                "x = 1; /* S */\nnoise\n/* E */\n\nnext();\n",
                "x = 1; \n\nnext();\n",
            );
            // Code after the end marker stays on its own line.
            check(
                "keep\n\n/* S */\nnoise\n/* E */ next();\n",
                "keep\n\n next();\n",
            );
            // A block inside one line of code.
            check("a /* S */ x /* E */ b\n", "a  b\n");
            // A block with its lines to itself goes with its indent.
            check(
                "    one();\n    /* S */\n    noise\n    /* E */\n    two();\n",
                "    one();\n    two();\n",
            );
            // The first end marker closes the block even when code follows it.
            check(
                "/* S */ a /* E */ code();\nkeep\n/* E */\n",
                " code();\nkeep\n/* E */\n",
            );
        }
    }

    #[test]
    fn test_block_with_empty_markers_removes_nothing() {
        assert_eq!(remove_block("one\n\n\ntwo\n", "", ""), "one\n\n\ntwo\n");
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
        let expected = re.replace_all("foo1 foo22 keep\nfoo333\nplain\n", replacement);
        assert_eq!(fs::read_to_string(&file).unwrap(), expected);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "bar1 bar22 keep\nbar333\nplain\n"
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
    fn test_trim_whitespace_preset_strips_crlf_lines_and_keeps_crlf() {
        // Verified failure: (?m)$ only matches before \n, so the trailing
        // \r on CRLF lines blocked every match ("code  \r\n" -> 0 edits).
        // (?mR) treats \r\n as a line boundary; $ sits before the \r,
        // which must stay in place.
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.rs");
        fs::write(&file, "code  \r\nmore\t\t\r\nclean\r\n").unwrap();

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
            "trailing whitespace on CRLF lines must be matched (anchors need (?mR))"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "code\r\nmore\r\nclean\r\n",
            "trimming must not swallow the \\r of a CRLF ending"
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
    fn test_remove_empty_comments_preset_matches_crlf_lines() {
        // Verified failure: (?m)^/$ never matched a lone "//" CRLF line
        // because the trailing \r blocked $. (?mR) treats \r\n as a line
        // boundary; the comment text goes, its CRLF ending stays.
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.js");
        fs::write(&file, "code line\r\n//\r\nmore code\r\n").unwrap();

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
            "a lone // line in a CRLF file must be matched (anchors need (?mR))"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "code line\r\n\r\nmore code\r\n"
        );
    }

    #[test]
    fn test_remove_empty_comments_preset_removes_multiline_empty_blocks() {
        // Verified regression vs main: deleting the preset's broken block
        // entry also dropped removal of two-line empty block comments
        // ("/*\n */") — 0 edits. A whole-content /\*\s*\*/ pattern covers
        // them, since patterns run against the whole file and \s spans lines.
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.js");
        fs::write(&file, "code\n/*\n */\nmore\n").unwrap();

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
            "a multi-line empty block comment must be removed"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "code\n\nmore\n");
    }

    /// Golden input for the `remove-todos` and `clean-debug` presets, in LF.
    /// It mixes lines a preset must change with lines it must leave alone;
    /// the CRLF variant is derived from it so the two cannot drift.
    const PRESET_GOLDEN_INPUT: &str = r##"function run(items, opts) {
  // TODO: split this function
  console.log("start");
  console.debug(items.length)
  console.trace();
  debugger;
  items.forEach(x => console.log(x));
  console.log(a); return compute(b);
  if (opts.debuggerEnabled) { start(); }
  console.log(
    "multi-line",
    items
  );
  chargeCustomer(order); // TODO: retry
  const url = "http://XXX.example.com";
  const link = '<a href="#TODO">later</a>';
  // HACK: first of two
  // XXX second of two
  return items; // FIXME
}
def load(name):
    # FIXME: cache this
    logger.debug("loaded %s", name)
    print(name)  # DEBUG
    print(name)  # debug
    timeout = compute_timeout()  # TODO: tune
    return name
System.out.println(total); // DEBUG
Debug.Print(total)
## TODO
"##;

    /// Runs `preset` over `content` in a temp file and returns the result.
    fn apply_preset(preset: &Preset, content: &str) -> String {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("golden.txt");
        fs::write(&file, content).unwrap();

        Replacer::new(load_preset(preset))
            .unwrap()
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: false,
                    dry_run: false,
                },
            )
            .unwrap();

        fs::read_to_string(&file).unwrap()
    }

    /// What `remove-todos` must make of `PRESET_GOLDEN_INPUT`: comment-only
    /// TODO lines are gone with their line break, trailing TODO comments are
    /// cut with the whitespace before them, and every other line (code,
    /// debug calls, markers inside strings, `## TODO`) is byte-identical.
    const REMOVE_TODOS_GOLDEN_OUTPUT: &str = r##"function run(items, opts) {
  console.log("start");
  console.debug(items.length)
  console.trace();
  debugger;
  items.forEach(x => console.log(x));
  console.log(a); return compute(b);
  if (opts.debuggerEnabled) { start(); }
  console.log(
    "multi-line",
    items
  );
  chargeCustomer(order);
  const url = "http://XXX.example.com";
  const link = '<a href="#TODO">later</a>';
  return items;
}
def load(name):
    logger.debug("loaded %s", name)
    print(name)  # DEBUG
    print(name)  # debug
    timeout = compute_timeout()
    return name
System.out.println(total); // DEBUG
Debug.Print(total)
## TODO
"##;

    #[test]
    fn test_remove_todos_preset_golden_lf() {
        // Verified failure: every pattern began with `.*`, so the code in
        // front of a trailing TODO comment was deleted with it
        // (`chargeCustomer(order); // TODO: retry` became an empty line), and
        // `"http://XXX.example.com"`, `href="#TODO"` and `## TODO` lines
        // were emptied too.
        assert_eq!(
            apply_preset(&Preset::RemoveTodos, PRESET_GOLDEN_INPUT),
            REMOVE_TODOS_GOLDEN_OUTPUT
        );
    }

    #[test]
    fn test_remove_todos_preset_golden_crlf() {
        // Same golden pair with CRLF endings: comment-only lines go with
        // their \r\n, and every surviving line keeps its \r\n.
        assert_eq!(
            apply_preset(
                &Preset::RemoveTodos,
                &PRESET_GOLDEN_INPUT.replace('\n', "\r\n")
            ),
            REMOVE_TODOS_GOLDEN_OUTPUT.replace('\n', "\r\n")
        );
    }

    /// What `clean-debug` must make of `PRESET_GOLDEN_INPUT`: a debug call
    /// that is the whole statement on its line becomes a comment stub at the
    /// same indent, and every other line (a call sharing its line with other
    /// code, a multi-line call, `debuggerEnabled`, `logger.debug`,
    /// `Debug.Print`, the TODO comments) is byte-identical.
    const CLEAN_DEBUG_GOLDEN_OUTPUT: &str = r##"function run(items, opts) {
  // TODO: split this function
  // console.log();
  // console.debug()
  // console.trace();
  // debugger;
  items.forEach(x => console.log(x));
  console.log(a); return compute(b);
  if (opts.debuggerEnabled) { start(); }
  console.log(
    "multi-line",
    items
  );
  chargeCustomer(order); // TODO: retry
  const url = "http://XXX.example.com";
  const link = '<a href="#TODO">later</a>';
  // HACK: first of two
  // XXX second of two
  return items; // FIXME
}
def load(name):
    # FIXME: cache this
    logger.debug("loaded %s", name)
    # print() # DEBUG
    # print() # debug
    timeout = compute_timeout()  # TODO: tune
    return name
// System.out.println(); // DEBUG
Debug.Print(total)
## TODO
"##;

    #[test]
    fn test_clean_debug_preset_golden_lf() {
        // Verified failure: the patterns matched anywhere in a line and up
        // to its last `)`, so `items.forEach(x => console.log(x));` became
        // `items.forEach(x => // console.log();`, `console.log(a); return
        // compute(b);` lost its return, `opts.debuggerEnabled` became
        // `opts.// debugger;Enabled`, and Python `logger.debug(...)` got a
        // `//` stub, which is not a comment in Python.
        assert_eq!(
            apply_preset(&Preset::CleanDebug, PRESET_GOLDEN_INPUT),
            CLEAN_DEBUG_GOLDEN_OUTPUT
        );
    }

    #[test]
    fn test_clean_debug_preset_golden_crlf() {
        // Same golden pair with CRLF endings: stubs replace the statement
        // only, and every line keeps its \r\n.
        assert_eq!(
            apply_preset(
                &Preset::CleanDebug,
                &PRESET_GOLDEN_INPUT.replace('\n', "\r\n")
            ),
            CLEAN_DEBUG_GOLDEN_OUTPUT.replace('\n', "\r\n")
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
            inputs: vec![dir.to_path_buf()],
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
            inputs: vec![dir.to_path_buf()],
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
            inputs: vec![dir.to_path_buf()],
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

        let undo_result = Replacer::undo(
            dir,
            UndoOptions {
                keep_backups: true,
                ..ALL_BAK
            },
        );

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let stats = undo_result.expect("an unreadable subdirectory must not fail undo");
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "original");

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let clean_result = run_clean_backups(dir.to_path_buf(), false, true);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        clean_result.expect("an unreadable subdirectory must not fail clean-backups");
        assert!(!dir.join("a.txt.bak").exists());
    }

    #[test]
    fn test_undo_drops_the_backup_record_and_keep_backups_refreshes_it() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        let backup = dir.join("t.txt.bak");
        fs::write(&file, "alpha\n").unwrap();

        let replacer = single_pattern_replacer("alpha", "longer text");
        let options = ProcessOptions {
            create_backup: true,
            dry_run: false,
        };
        replacer.process_file(&file, options).unwrap();
        let record = backup_records::load(&backup).expect("replace must record its backup");
        assert_eq!(record.size, fs::metadata(&file).unwrap().len());

        // A dry run touches neither the files nor the record.
        let stats = Replacer::undo(
            dir,
            UndoOptions {
                dry_run: true,
                ..RECORDED_ONLY
            },
        )
        .unwrap();
        assert_eq!((stats.found, stats.restored), (1, 0));
        assert_eq!(fs::read_to_string(&file).unwrap(), "longer text\n");
        assert!(backup_records::load(&backup).is_some());

        // --keep-backups keeps the record, describing the restored file.
        let stats = Replacer::undo(
            dir,
            UndoOptions {
                keep_backups: true,
                ..RECORDED_ONLY
            },
        )
        .unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
        let kept = backup_records::load(&backup).expect("--keep-backups must keep the record");
        assert_eq!(kept.size, 6);
        assert_eq!(kept.mtime, fs::metadata(&file).unwrap().modified().unwrap());
        assert_eq!(kept.created, record.created);

        // So the next replace still sees its own untouched backup.
        replacer.process_file(&file, options).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "longer text\n");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "alpha\n");

        // A plain undo consumes the backup and its record.
        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!(stats.restored, 1);
        assert!(!backup.exists());
        assert!(backup_records::load(&backup).is_none());

        // A record whose backup was deleted by hand is dropped, not an error.
        replacer.process_file(&file, options).unwrap();
        fs::remove_file(&backup).unwrap();
        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.failed), (0, 0));
        assert!(backup_records::load(&backup).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_undo_keeps_the_record_of_a_backup_it_cannot_reach() {
        use std::os::unix::fs::PermissionsExt;

        // Verified failure: a recorded backup inside an unreadable directory
        // was taken for a deleted one and its record was dropped, so undo
        // skipped the backup for good once the directory was readable again.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let locked = dir.join("locked");
        fs::create_dir(&locked).unwrap();
        let file = locked.join("t.txt");
        let backup = locked.join("t.txt.bak");
        fs::write(&file, "alpha\n").unwrap();
        single_pattern_replacer("alpha", "beta")
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: true,
                    dry_run: false,
                },
            )
            .unwrap();

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let result = Replacer::undo(dir, RECORDED_ONLY);
        // Restore permissions before asserting so TempDir cleanup succeeds.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        let stats = result.unwrap();
        assert_eq!((stats.restored, stats.failed), (0, 1));
        assert!(
            backup_records::load(&backup).is_some(),
            "the record of an unreachable backup must be kept"
        );

        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
    }

    #[test]
    fn test_undo_drops_the_record_of_a_backup_whose_directory_is_gone() {
        // Verified failure: the record was looked up through the backup's
        // parent directory, so once that directory was deleted the record
        // could never be removed.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        let file = sub.join("t.txt");
        fs::write(&file, "alpha\n").unwrap();
        single_pattern_replacer("alpha", "beta")
            .process_file(
                &file,
                ProcessOptions {
                    create_backup: true,
                    dry_run: false,
                },
            )
            .unwrap();
        fs::remove_dir_all(&sub).unwrap();

        let canonical_dir = fs::canonicalize(dir).unwrap();
        assert_eq!(backup_records::list_under(&canonical_dir).len(), 1);

        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.failed), (0, 0));
        assert!(backup_records::list_under(&canonical_dir).is_empty());
    }

    /// Runs `alpha` -> `beta` over `file` with a backup, then swaps the
    /// backup oober made for a file of the user's own. Returns the backup
    /// path. The user's file differs in size, so no test here depends on the
    /// clock's resolution.
    fn replace_then_swap_backup(file: &Path) -> PathBuf {
        fs::write(file, "alpha\n").unwrap();
        single_pattern_replacer("alpha", "beta")
            .process_file(
                file,
                ProcessOptions {
                    create_backup: true,
                    dry_run: false,
                },
            )
            .unwrap();
        let mut backup = file.as_os_str().to_owned();
        backup.push(".bak");
        let backup = PathBuf::from(backup);
        fs::remove_file(&backup).unwrap();
        fs::write(&backup, "my own copy\n").unwrap();
        backup
    }

    #[test]
    fn test_a_file_put_at_a_recorded_backup_path_is_not_oobers_backup() {
        // Verified failure: a record named only the backup's path. After the
        // backup was deleted by hand, any file created at that path counted
        // as oober's: undo restored it over the live file and deleted it,
        // and clean-backups deleted it.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        let backup = replace_then_swap_backup(&file);
        fs::write(&file, "edited by hand\n").unwrap();

        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.restored, stats.unrecorded), (0, 0, 1));
        run_clean_backups(dir.to_path_buf(), false, false).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "edited by hand\n");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "my own copy\n");

        // --all-bak still restores it, and --keep-backups must not turn the
        // user's file into a recorded backup on the way.
        let stats = Replacer::undo(
            dir,
            UndoOptions {
                keep_backups: true,
                ..ALL_BAK
            },
        )
        .unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "my own copy\n");
        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.unrecorded), (0, 1));
        assert_eq!(fs::read_to_string(&backup).unwrap(), "my own copy\n");
    }

    #[test]
    fn test_replace_refuses_a_file_whose_recorded_backup_was_swapped() {
        // Verified failure: the live file still matched the record, so the
        // second run kept the user's file as "the oldest backup", changed
        // the live file, and undo then restored the user's file over it.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        let backup = replace_then_swap_backup(&file);

        let result = single_pattern_replacer("beta", "gamma").process_file(
            &file,
            ProcessOptions {
                create_backup: true,
                dry_run: false,
            },
        );
        assert!(matches!(result, Err(Error::BackupExists(_))));
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");

        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.restored), (0, 0));
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "my own copy\n");
    }

    #[test]
    fn test_a_recorded_backup_with_another_mtime_is_not_oobers_backup() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        let backup = replace_then_swap_backup(&file);

        // The same size as the backup oober made ("alpha\n"), one minute
        // newer: only the modification time tells the two apart.
        let recorded = backup_records::load(&backup).unwrap();
        fs::write(&backup, "ALPHA\n").unwrap();
        fs::File::options()
            .write(true)
            .open(&backup)
            .unwrap()
            .set_modified(recorded.created + std::time::Duration::from_secs(60))
            .unwrap();

        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.unrecorded), (0, 1));
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "ALPHA\n");
    }

    #[test]
    fn test_a_swapped_backup_the_walk_does_not_reach_is_left_alone() {
        // The backup of a hidden input is found through its record, not by
        // the walk, so it needs the same check there.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join(".env");
        let backup = replace_then_swap_backup(&file);

        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.restored, stats.failed), (0, 0, 0));
        run_clean_backups(dir.to_path_buf(), false, false).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "my own copy\n");
    }

    #[cfg(unix)]
    #[test]
    fn test_clean_backups_leaves_a_symlink_put_at_a_recorded_backup_path() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let file = dir.join("t.txt");
        let backup = replace_then_swap_backup(&file);
        fs::remove_file(&backup).unwrap();
        std::os::unix::fs::symlink(dir.join("nowhere"), &backup).unwrap();

        run_clean_backups(dir.to_path_buf(), false, false).unwrap();
        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!((stats.found, stats.failed), (0, 0));
        assert!(
            fs::symlink_metadata(&backup)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the user's symlink must be left in place"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");
    }

    #[cfg(unix)]
    #[test]
    fn test_undo_refuses_to_restore_through_or_from_a_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("target.txt");
        fs::write(&target, "outside").unwrap();

        // The original's path holds a symlink: restoring must not replace
        // the link or write through it.
        std::os::unix::fs::symlink(&target, dir.join("a.txt")).unwrap();
        fs::write(dir.join("a.txt.bak"), "backup").unwrap();
        // The backup itself is a symlink: its target is not oober's backup.
        fs::write(dir.join("b.txt"), "live").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("b.txt.bak")).unwrap();

        let stats = Replacer::undo(dir, ALL_BAK).unwrap();
        assert_eq!((stats.found, stats.restored, stats.failed), (2, 0, 2));
        assert!(
            fs::symlink_metadata(dir.join("a.txt"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "outside");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "live");
        assert!(dir.join("a.txt.bak").exists());
        assert!(
            fs::symlink_metadata(dir.join("b.txt.bak"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
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
        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
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
                inputs: vec![dir.to_path_buf()],
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
        let stats = Replacer::undo(dir, RECORDED_ONLY).unwrap();
        assert_eq!(stats.restored, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
    }

    #[cfg(unix)]
    #[test]
    fn test_replace_never_processes_symlink_to_file_in_tree() {
        // docs/README.md -> ../README.md. Whichever spelling is reached first,
        // the link must stay a link and the real file must be transformed
        // exactly once (X -> XX applied twice would leave XXXX). Input order
        // fixes which one is reached first; the last case leaves it to the
        // walk.
        type Inputs = fn(&Path) -> Vec<PathBuf>;
        let cases: [(&str, Inputs); 3] = [
            ("link first", |root| {
                vec![root.join("docs"), root.to_path_buf()]
            }),
            ("target first", |root| {
                vec![root.join("README.md"), root.to_path_buf()]
            }),
            ("walk order", |root| vec![root.to_path_buf()]),
        ];

        for (case, inputs) in cases {
            let temp_dir = TempDir::new().unwrap();
            let root = temp_dir.path();
            let target = root.join("README.md");
            fs::write(&target, "X\n").unwrap();
            fs::create_dir(root.join("docs")).unwrap();
            let link = root.join("docs").join("README.md");
            std::os::unix::fs::symlink("../README.md", &link).unwrap();

            run_replace(ReplaceOptions {
                preset: None,
                config_file: None,
                pattern: Some("X".into()),
                replacement: Some("XX".into()),
                inputs: inputs(root),
                extensions: vec![],
                exclude: vec![],
                no_backup: true,
                dry_run: false,
                verbose: false,
                workers: Some(1),
            })
            .unwrap();

            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{case}: the symlink must not be replaced by a regular file"
            );
            assert_eq!(
                fs::read_to_string(&target).unwrap(),
                "XX\n",
                "{case}: the real file must be transformed exactly once"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_explicit_symlink_input_is_skipped_and_target_untouched() {
        // The target lives outside the processed inputs, so only the link
        // could reach it.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let target = dir.join("real.txt");
        fs::write(&target, "X\n").unwrap();
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        run_replace(ReplaceOptions {
            preset: None,
            config_file: None,
            pattern: Some("X".into()),
            replacement: Some("XX".into()),
            inputs: vec![link.clone()],
            extensions: vec![],
            exclude: vec![],
            no_backup: true,
            dry_run: false,
            verbose: false,
            workers: Some(1),
        })
        .unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "an explicit symlink input must stay a symlink"
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "X\n",
            "the target of an explicit symlink input must not be modified"
        );
    }
}
