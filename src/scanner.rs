use crate::config::{ConfigLoader, Pattern};
use crate::errors::Result;
use crate::fingerprint::{FileFingerprint, Fingerprinter};
use crate::output_formatter::{OutputFormat, OutputFormatter};
use crate::state_manager::{CACHE_FORMAT, CachedMatch, FileState, ScanState, StateManager};
use crate::util::{
    normalize_extensions, require_valid_extensions, resolve_workers, should_process_file,
};
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::RegexSet;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::SystemTime;

/// The core engine for scanning files for regex patterns.
///
/// A `Scanner` is initialized with a set of named patterns. It uses a `RegexSet`
/// for efficient matching of multiple patterns against lines of text; the set's
/// matched indices identify which named patterns hit each line (no per-pattern
/// `Regex` re-confirmation is performed).
pub struct Scanner {
    pattern_names: Vec<String>,
    pattern_set: RegexSet,
}

/// Represents a single occurrence of a matched pattern in a file.
#[derive(Debug, Clone)]
pub struct Match {
    /// The name of the pattern that was matched.
    pub pattern_name: String,
    /// The path to the file where the match was found.
    pub file_path: PathBuf,
    /// The line number of the match.
    pub line_number: usize,
    /// The content of the line that contained the match.
    pub line_content: String,
}

impl Scanner {
    /// Creates a new `Scanner` from a vector of `Pattern`s.
    ///
    /// This function compiles all the provided patterns into a `RegexSet` for
    /// efficient multi-pattern matching.
    pub fn new(patterns: Vec<Pattern>) -> Result<Self> {
        let mut pattern_strings = Vec::new();
        let mut pattern_names = Vec::new();

        for p in patterns.into_iter() {
            pattern_strings.push(p.pattern);
            pattern_names.push(p.name);
        }

        let pattern_set = RegexSet::new(&pattern_strings)?;

        Ok(Self {
            pattern_names,
            pattern_set,
        })
    }

    /// Scans a single file for all configured patterns.
    ///
    /// It reads the file and checks each line against the `RegexSet`; every
    /// pattern index the set reports produces a `Match` for that line.
    ///
    /// This function includes a simple heuristic to skip binary files by checking for
    /// null bytes in the first 1KB of the file.
    pub fn scan_file(&self, path: &Path) -> Result<Vec<Match>> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Basic binary detection: check for null bytes in the first 1024 bytes (peek only).
        let buf = reader.fill_buf()?;
        let sample_len = buf.len().min(1024);
        if buf[..sample_len].contains(&0) {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();
        let mut line_buf = Vec::new();
        let mut line_number = 0usize;

        loop {
            line_buf.clear();
            let bytes_read = reader.read_until(b'\n', &mut line_buf)?;
            if bytes_read == 0 {
                break;
            }
            line_number += 1;
            if line_buf.last() == Some(&b'\n') {
                line_buf.pop();
            }

            let line_cow = match std::str::from_utf8(&line_buf) {
                Ok(s) => Cow::Borrowed(s),
                Err(_) => Cow::Owned(String::from_utf8_lossy(&line_buf).into_owned()),
            };

            let match_set = self.pattern_set.matches(line_cow.as_ref());
            if match_set.matched_any() {
                let mut iter = match_set.iter();
                if let Some(first_idx) = iter.next() {
                    let line_content = line_cow.into_owned();
                    let name = &self.pattern_names[first_idx];
                    matches.push(Match {
                        pattern_name: name.clone(),
                        file_path: path.to_path_buf(),
                        line_number,
                        line_content: line_content.clone(),
                    });
                    for pattern_idx in iter {
                        let name = &self.pattern_names[pattern_idx];
                        matches.push(Match {
                            pattern_name: name.clone(),
                            file_path: path.to_path_buf(),
                            line_number,
                            line_content: line_content.clone(),
                        });
                    }
                }
            }
        }

        Ok(matches)
    }

    /// Scans a directory for files matching the given extensions and finds pattern matches.
    ///
    /// Hidden files and gitignored files are skipped unless `hidden` /
    /// `no_ignore` are set (see [`build_walker`]).
    ///
    /// This is a sequential, single-threaded scan. For better performance on large
    /// directories, use `scan_directory_parallel`.
    pub fn scan_directory(
        &self,
        dir: &Path,
        extensions: &[String],
        hidden: bool,
        no_ignore: bool,
    ) -> Result<Vec<Match>> {
        let extensions = normalize_extensions(extensions);
        self.scan_directory_with_set(dir, &extensions, hidden, no_ignore)
    }

    fn scan_directory_with_set(
        &self,
        dir: &Path,
        extensions: &HashSet<String>,
        hidden: bool,
        no_ignore: bool,
    ) -> Result<Vec<Match>> {
        let mut all_matches = Vec::new();

        for entry in build_walker(dir, hidden, no_ignore).build() {
            // A single unreadable entry must not kill the whole scan.
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("Warning: skipping unreadable path: {e}");
                    continue;
                }
            };
            let path = entry.path();
            if path.is_file() && should_process_file(path, extensions) {
                // A single unreadable file must not kill the whole scan.
                match self.scan_file(path) {
                    Ok(matches) => all_matches.extend(matches),
                    Err(e) => {
                        eprintln!(
                            "Warning: skipping unreadable file '{}': {e}",
                            path.display()
                        )
                    }
                }
            }
        }

        Ok(all_matches)
    }

    /// Scans a directory in parallel using a Rayon thread pool.
    ///
    /// This function is significantly faster than `scan_directory` for large numbers
    /// of files. It first collects all files to be scanned and then distributes the
    /// scanning work across the thread pool.
    pub fn scan_directory_parallel(
        &self,
        dir: &Path,
        extensions: &[String],
        workers: Option<usize>,
        hidden: bool,
        no_ignore: bool,
    ) -> Result<Vec<Match>> {
        let extensions = normalize_extensions(extensions);
        self.scan_directory_parallel_with_set(dir, &extensions, workers, hidden, no_ignore)
    }

    fn scan_directory_parallel_with_set(
        &self,
        dir: &Path,
        extensions: &HashSet<String>,
        workers: Option<usize>,
        hidden: bool,
        no_ignore: bool,
    ) -> Result<Vec<Match>> {
        // Collect all file paths first
        let files: Vec<PathBuf> = build_walker(dir, hidden, no_ignore)
            .threads(resolve_workers(workers))
            .build()
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry),
                Err(e) => {
                    // A single unreadable entry must not kill the whole scan.
                    eprintln!("Warning: skipping unreadable path: {e}");
                    None
                }
            })
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && should_process_file(path, extensions)
            })
            .map(|entry| entry.path().to_path_buf())
            .collect();

        Ok(self.scan_files_parallel(&files, workers)?.0)
    }

    /// Scans a list of files in parallel using a local Rayon thread pool.
    ///
    /// Returns the matches together with the paths that could not be scanned,
    /// so callers can tell a file without matches from a file that was never
    /// read.
    fn scan_files_parallel(
        &self,
        files: &[PathBuf],
        workers: Option<usize>,
    ) -> Result<(Vec<Match>, Vec<PathBuf>)> {
        if files.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(resolve_workers(workers))
            .build()?;

        Ok(pool.install(|| {
            files
                .par_iter()
                .fold(
                    || (Vec::new(), Vec::new()),
                    |mut acc, path| {
                        // A single unreadable file must not kill the whole scan.
                        match self.scan_file(path) {
                            Ok(matches) => acc.0.extend(matches),
                            Err(e) => {
                                eprintln!(
                                    "Warning: skipping unreadable file '{}': {e}",
                                    path.display()
                                );
                                acc.1.push(path.clone());
                            }
                        }
                        acc
                    },
                )
                .reduce(
                    || (Vec::new(), Vec::new()),
                    |mut left, mut right| {
                        left.0.append(&mut right.0);
                        left.1.append(&mut right.1);
                        left
                    },
                )
        }))
    }
}

/// Options for [`run_scan`], mirroring the flags of the `scan` subcommand.
pub struct ScanOptions {
    /// Path to the YAML file defining the scan patterns.
    pub patterns_file: PathBuf,
    /// Inline regexes; when non-empty, the patterns file is ignored.
    pub inline_patterns: Vec<String>,
    /// Output file path. `None` writes to standard output.
    pub output: Option<PathBuf>,
    /// File extensions to include. Empty means all files.
    pub extensions: Vec<String>,
    /// The input files or directories to scan.
    pub inputs: Vec<PathBuf>,
    /// Worker thread count. `None` uses the number of logical CPU cores.
    pub workers: Option<usize>,
    /// Only re-scan files that changed since the cached state.
    pub incremental: bool,
    /// Force a full re-scan, ignoring any cached results.
    pub force_full: bool,
    /// Clear the cache before scanning.
    pub clear_cache: bool,
    /// Use content hashes instead of timestamps for change detection.
    pub content_hash: bool,
    /// Do not persist matched line contents in the incremental cache.
    pub no_cache_content: bool,
    /// The output format for the scan results.
    pub format: OutputFormat,
    /// Append a summary of scan statistics (text format only).
    pub include_summary: bool,
    /// Include hidden files and directories.
    pub hidden: bool,
    /// Do not honor .gitignore/.ignore files.
    pub no_ignore: bool,
}

/// The main entry point for the `scan` command.
///
/// This function handles:
/// 1. Loading the scan patterns from a configuration file.
/// 2. Setting up the output writer (either a file or `stdout`).
/// 3. Iterating through the input paths and dispatching to the appropriate
///    `Scanner` methods (for files or directories).
/// 4. Writing the results.
pub fn run_scan(options: ScanOptions) -> Result<()> {
    let ScanOptions {
        patterns_file,
        inline_patterns,
        output,
        extensions,
        inputs,
        workers,
        incremental,
        force_full,
        clear_cache,
        content_hash,
        no_cache_content,
        format,
        include_summary,
        hidden,
        no_ignore,
    } = options;

    // A summary is only rendered for the text format; reject the combination
    // instead of silently ignoring the flag.
    if include_summary && !matches!(format, OutputFormat::Text) {
        return Err("--summary is only supported with --format text".into());
    }

    // The flag only changes what the incremental cache stores; reject it
    // without --incremental instead of silently ignoring it.
    if no_cache_content && !incremental {
        return Err("--no-cache-content is only supported with --incremental".into());
    }

    let exts = require_valid_extensions(&extensions)?;

    // Load patterns: inline -e flags take precedence over the patterns file.
    let patterns: Vec<Pattern> = if !inline_patterns.is_empty() {
        inline_patterns
            .into_iter()
            .enumerate()
            .map(|(i, pattern)| Pattern {
                name: format!("inline_{}", i + 1),
                pattern,
            })
            .collect()
    } else {
        // The patterns file resolves ONLY as given (absolute, or relative to
        // the current directory): a typo'd path must be an error, never a
        // silent fallback to a same-named file elsewhere (e.g. a stale copy
        // in $XDG_CONFIG_HOME/oober), which for a security scan would be a
        // false all-clear.
        let loaded = ConfigLoader::load_scan_config(&patterns_file)?.patterns;
        // Report on stderr which file is used; stdout carries the results.
        eprintln!("Using patterns file: {}", patterns_file.display());
        loaded
    };
    let patterns_hash = compute_patterns_hash(&patterns);

    // Create scanner
    let scanner = Arc::new(Scanner::new(patterns)?);

    // Prepare output
    let mut writer: Box<dyn Write + Send> = match output {
        Some(path) => Box::new(BufWriter::new(File::create(path)?)),
        None => Box::new(BufWriter::new(std::io::stdout())),
    };

    let files = collect_files(&inputs, &exts, hidden, no_ignore)?;
    let project_root = determine_project_root(&inputs)?;
    let mut files_to_scan = files.clone();
    let mut cached_matches: Vec<Match> = Vec::new();

    if clear_cache && !incremental {
        let manager = StateManager::new(&project_root)?;
        manager.clear_cache()?;
    }

    let mut state_manager: Option<StateManager> = None;
    let mut fingerprinter: Option<Fingerprinter> = None;
    // Fingerprints learned during change detection, carried into state saving
    // so no file is fingerprinted (read, in content-hash mode) twice per run.
    let mut known_fingerprints: HashMap<PathBuf, FileFingerprint> = HashMap::new();

    if incremental {
        let manager = StateManager::new(&project_root)?;

        if clear_cache {
            manager.clear_cache()?;
        }

        let mut cached_state = if !force_full && !clear_cache {
            manager.load_state()?
        } else {
            None
        };

        if let Some(ref state) = cached_state {
            if state.patterns_hash != patterns_hash {
                cached_state = None;
            }
        }

        let fp = Fingerprinter::new(content_hash);

        if let Some(state) = &cached_state {
            let mut changed_files = Vec::new();

            for file in &files {
                if let Some(file_state) = state.files.get(file) {
                    // A file that cannot be checked must not end the scan:
                    // treat it as changed, so it is scanned like any other.
                    let (changed, fingerprint) = match fp.check_file(file, file_state) {
                        Ok(checked) => checked,
                        Err(e) => {
                            eprintln!(
                                "Warning: could not check '{}' for changes: {e}; re-scanning",
                                file.display()
                            );
                            (true, None)
                        }
                    };
                    if !changed {
                        // Unchanged: the cached fingerprint is still valid, so
                        // state saving must not read the file again.
                        known_fingerprints.insert(
                            file.to_path_buf(),
                            FileFingerprint {
                                path: file.to_path_buf(),
                                modified: file_state.modified,
                                size: file_state.size,
                                hash: file_state.hash.clone(),
                            },
                        );
                        if let Some(cached) = state.scan_results.get(file) {
                            match resolve_cached_matches(file, cached) {
                                Ok(resolved) => cached_matches.extend(resolved),
                                Err(e) => {
                                    // Content-less cache entries (written under
                                    // --no-cache-content) could not be turned
                                    // back into matches; re-scan the file fresh.
                                    eprintln!(
                                        "Warning: could not reconstruct cached matches for '{}': {e}; re-scanning",
                                        file.display()
                                    );
                                    changed_files.push(file.to_path_buf());
                                }
                            }
                        }
                        continue;
                    }
                    // Changed: reuse the fingerprint computed during detection,
                    // if the comparison got far enough to produce one.
                    if let Some(fingerprint) = fingerprint {
                        known_fingerprints.insert(file.to_path_buf(), fingerprint);
                    }
                }

                changed_files.push(file.to_path_buf());
            }

            files_to_scan = changed_files;
        }

        state_manager = Some(manager);
        fingerprinter = Some(fp);
    }

    let can_stream = matches!(format, OutputFormat::Text) && !include_summary && !incremental;

    if can_stream {
        stream_text_output(scanner, &files_to_scan, workers, writer)?;
        return Ok(());
    }

    let mut all_matches = Vec::new();
    if !cached_matches.is_empty() {
        all_matches.extend(cached_matches);
    }

    let (scanned_matches, failed_files) = scanner.scan_files_parallel(&files_to_scan, workers)?;
    all_matches.extend(scanned_matches);

    let formatter = OutputFormatter::new(format, include_summary);
    formatter.write_output(&mut writer, &all_matches)?;

    if incremental {
        let manager = state_manager.expect("State manager missing");
        let fp = fingerprinter.expect("Fingerprinter missing");
        let scan_state = build_scan_state(
            &files,
            known_fingerprints,
            &all_matches,
            &failed_files,
            &fp,
            patterns_hash,
            no_cache_content,
        );
        manager.save_state(&scan_state)?;
    }

    Ok(())
}

/// Builds a directory walker for scanning. The standard filters (hidden
/// files, .gitignore/.ignore rules) are on by default; `hidden` re-includes
/// hidden files and directories, and `no_ignore` stops honoring
/// .gitignore/.ignore files (ripgrep-style `--hidden` / `--no-ignore`).
fn build_walker(dir: &Path, hidden: bool, no_ignore: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(dir);
    builder.standard_filters(true);
    if hidden {
        builder.hidden(false);
    }
    if no_ignore {
        builder
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .parents(false);
    }
    builder
}

fn collect_files(
    inputs: &[PathBuf],
    extensions: &HashSet<String>,
    hidden: bool,
    no_ignore: bool,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    // A file reached twice (named explicitly and found in a directory walk,
    // or via overlapping directories) must be scanned once: listed twice it
    // reports every match twice, and an incremental run doubles its cached
    // matches each time. Paths are deduplicated by canonical form so
    // different spellings of one file collapse; the first spelling is kept.
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let canonical =
        |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Canonical form of each walked directory, resolved once per directory.
    let mut canonical_dirs: HashMap<PathBuf, PathBuf> = HashMap::new();

    for input in inputs {
        if input.is_file() {
            if should_process_file(input, extensions) && seen.insert(canonical(input)) {
                files.push(input.to_path_buf());
            }
        } else if input.is_dir() {
            for entry in build_walker(input, hidden, no_ignore).build() {
                // A single unreadable entry (e.g. a permission-denied
                // subdirectory) must not kill the whole scan.
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        eprintln!("Warning: skipping unreadable path: {e}");
                        continue;
                    }
                };
                let path = entry.path();
                if path.is_file() && should_process_file(path, extensions) {
                    // Resolving every file costs one realpath per file. A
                    // file that is not a symlink is its canonical directory
                    // plus its name, so only symlinks are resolved in full.
                    let key = match (path.parent(), path.file_name()) {
                        (Some(parent), Some(name)) if !entry.path_is_symlink() => canonical_dirs
                            .entry(parent.to_path_buf())
                            .or_insert_with(|| canonical(parent))
                            .join(name),
                        _ => canonical(path),
                    };
                    if seen.insert(key) {
                        files.push(path.to_path_buf());
                    }
                }
            }
        } else {
            return Err(format!("Input path not found: {}", input.display()).into());
        }
    }

    Ok(files)
}

fn determine_project_root(inputs: &[PathBuf]) -> Result<PathBuf> {
    if let Some(dir) = inputs.iter().find(|p| p.is_dir()) {
        return Ok(dir.to_path_buf());
    }

    if let Some(file) = inputs.iter().find(|p| p.is_file()) {
        if let Some(parent) = file.parent() {
            return Ok(parent.to_path_buf());
        }
    }

    Ok(std::env::current_dir()?)
}

fn compute_patterns_hash(patterns: &[Pattern]) -> String {
    let mut hasher = Sha256::new();
    for pattern in patterns {
        hasher.update(pattern.name.as_bytes());
        hasher.update(b"\0");
        hasher.update(pattern.pattern.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

/// Builds the state to cache for this run.
///
/// A file in `failed` could not be scanned and is left out, even when a
/// fingerprint was carried over for it: saved, it would count as scanned
/// with no matches and be skipped until its size or modification time
/// changed. A file that cannot be fingerprinted is left out with a warning
/// instead of failing the run. Either way the next run scans the file again.
fn build_scan_state(
    files: &[PathBuf],
    mut known_fingerprints: HashMap<PathBuf, FileFingerprint>,
    matches: &[Match],
    failed: &[PathBuf],
    fingerprinter: &Fingerprinter,
    patterns_hash: String,
    no_cache_content: bool,
) -> ScanState {
    let now = SystemTime::now();
    let failed: HashSet<&PathBuf> = failed.iter().collect();
    let mut file_states = HashMap::new();

    for file in files {
        if failed.contains(file) {
            continue;
        }
        // Reuse the fingerprint carried over from change detection; only files
        // without one (new files, or size-mismatch short circuits) are read here.
        let fingerprint = match known_fingerprints.remove(file) {
            Some(fingerprint) => fingerprint,
            None => match fingerprinter.fingerprint_file(file) {
                Ok(fingerprint) => fingerprint,
                Err(e) => {
                    eprintln!(
                        "Warning: not caching '{}': could not fingerprint it: {e}",
                        file.display()
                    );
                    continue;
                }
            },
        };
        file_states.insert(
            file.to_path_buf(),
            FileState {
                path: file.to_path_buf(),
                modified: fingerprint.modified,
                size: fingerprint.size,
                hash: fingerprint.hash,
                last_scanned: now,
            },
        );
    }

    ScanState {
        version: env!("CARGO_PKG_VERSION").to_string(),
        format: CACHE_FORMAT,
        last_scan: now,
        files: file_states,
        patterns_hash,
        scan_results: build_scan_results(matches, no_cache_content),
    }
}

fn build_scan_results(
    matches: &[Match],
    no_cache_content: bool,
) -> HashMap<PathBuf, Vec<CachedMatch>> {
    let mut results: HashMap<PathBuf, Vec<CachedMatch>> = HashMap::new();

    for m in matches {
        results
            .entry(m.file_path.clone())
            .or_default()
            .push(CachedMatch {
                pattern_name: m.pattern_name.clone(),
                line_number: m.line_number,
                // Under --no-cache-content the matched text stays out of the
                // on-disk cache; only the location is persisted.
                line_content: if no_cache_content {
                    String::new()
                } else {
                    m.line_content.clone()
                },
            });
    }

    results
}

/// Turns a fingerprint-unchanged file's cached matches back into [`Match`]es.
///
/// Entries whose `line_content` is empty (written under `--no-cache-content`)
/// are reconstructed by re-reading just the recorded line numbers from the
/// file in a single pass. Fails if the file cannot be read or a recorded line
/// no longer exists — despite the unchanged fingerprint — so the caller can
/// fall back to re-scanning the file fresh.
fn resolve_cached_matches(file: &Path, cached: &[CachedMatch]) -> Result<Vec<Match>> {
    let needed: HashSet<usize> = cached
        .iter()
        .filter(|m| m.line_content.is_empty())
        .map(|m| m.line_number)
        .collect();

    let mut reconstructed: HashMap<usize, String> = HashMap::new();
    if let Some(&max_needed) = needed.iter().max() {
        let mut reader = BufReader::new(File::open(file)?);
        let mut line_buf = Vec::new();
        let mut line_number = 0usize;
        while line_number < max_needed {
            line_buf.clear();
            let bytes_read = reader.read_until(b'\n', &mut line_buf)?;
            if bytes_read == 0 {
                break;
            }
            line_number += 1;
            if !needed.contains(&line_number) {
                continue;
            }
            if line_buf.last() == Some(&b'\n') {
                line_buf.pop();
            }
            reconstructed.insert(line_number, String::from_utf8_lossy(&line_buf).into_owned());
        }
        if reconstructed.len() != needed.len() {
            // The loop inserts every needed line it reaches, so a shortfall
            // means EOF came first — max_needed is always among the missing.
            return Err(format!("cached line {max_needed} is beyond the end of the file").into());
        }
    }

    Ok(cached
        .iter()
        .map(|m| Match {
            pattern_name: m.pattern_name.clone(),
            file_path: file.to_path_buf(),
            line_number: m.line_number,
            line_content: match reconstructed.get(&m.line_number) {
                Some(content) => content.clone(),
                None => m.line_content.clone(),
            },
        })
        .collect())
}

fn stream_text_output(
    scanner: Arc<Scanner>,
    files: &[PathBuf],
    workers: Option<usize>,
    mut writer: Box<dyn Write + Send>,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<String>();

    let writer_handle = std::thread::spawn(move || -> Result<()> {
        for line in rx {
            writer.write_all(line.as_bytes())?;
        }
        writer.flush()?;
        Ok(())
    });

    let scan_result = if files.is_empty() {
        Ok(())
    } else {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(resolve_workers(workers))
            .build()?;

        pool.install(|| {
            files.par_iter().try_for_each(|path| -> Result<()> {
                // A single unreadable file must not kill the whole scan.
                let matches = match scanner.scan_file(path) {
                    Ok(matches) => matches,
                    Err(e) => {
                        eprintln!(
                            "Warning: skipping unreadable file '{}': {e}",
                            path.display()
                        );
                        return Ok(());
                    }
                };
                if matches.is_empty() {
                    return Ok(());
                }

                for m in matches {
                    let line = format!(
                        "[{}] {}:{}: {}\n",
                        m.pattern_name,
                        m.file_path.display(),
                        m.line_number,
                        m.line_content
                    );
                    tx.send(line).map_err(|_| "Output channel closed")?;
                }

                Ok(())
            })
        })
    };

    drop(tx);
    let writer_result = writer_handle
        .join()
        .unwrap_or_else(|_| Err("Output writer thread panicked".into()));

    scan_result?;
    writer_result?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_regex_set_matching() {
        let patterns = vec![
            Pattern {
                name: "email".into(),
                pattern: r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b".into(),
            },
            Pattern {
                name: "url".into(),
                pattern: r"https?://[^\s]+".into(),
            },
            Pattern {
                name: "ip".into(),
                pattern: r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b".into(),
            },
        ];

        let scanner = Scanner::new(patterns).unwrap();

        // Create temp file with test content
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        let test_content = "Contact: test@example.com\nVisit https://example.com\nServer: 192.168.1.1\nPlain text line\n";
        fs::write(&test_file, test_content).unwrap();

        let matches = scanner.scan_file(&test_file).unwrap();

        // Assert: 3 matches found, plain text line produces no matches
        assert_eq!(matches.len(), 3);
        assert!(matches.iter().any(|m| m.pattern_name == "email"));
        assert!(matches.iter().any(|m| m.pattern_name == "url"));
        assert!(matches.iter().any(|m| m.pattern_name == "ip"));
    }

    #[test]
    fn test_pattern_name_preservation() {
        let patterns = vec![Pattern {
            name: "test_pattern".into(),
            pattern: r"test".into(),
        }];

        let scanner = Scanner::new(patterns).unwrap();

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "this is a test").unwrap();

        let matches = scanner.scan_file(&test_file).unwrap();
        assert_eq!(matches[0].pattern_name, "test_pattern");
    }

    #[test]
    fn test_summary_with_non_text_format_is_rejected() {
        let temp_dir = TempDir::new().unwrap();

        let options = |format| ScanOptions {
            patterns_file: PathBuf::from("patterns.yaml"),
            inline_patterns: vec!["TODO".to_string()],
            output: None,
            extensions: vec![],
            inputs: vec![temp_dir.path().to_path_buf()],
            workers: Some(1),
            incremental: false,
            force_full: false,
            clear_cache: false,
            content_hash: false,
            no_cache_content: false,
            format,
            include_summary: true,
            hidden: false,
            no_ignore: false,
        };

        for format in [
            OutputFormat::Json,
            OutputFormat::Csv,
            OutputFormat::Sarif,
            OutputFormat::Html,
        ] {
            let result = run_scan(options(format));
            let err = result.expect_err("--summary must be rejected for non-text formats");
            assert!(
                err.to_string().contains("--summary"),
                "error must name --summary: {err}"
            );
        }

        // --summary with the text format must remain valid.
        run_scan(options(OutputFormat::Text)).unwrap();
    }

    #[test]
    fn test_all_invalid_extensions_filter_is_rejected() {
        // Verified failure: `-x .` normalized to an empty set, and an empty
        // set means "scan every file" — an explicit filter must never widen
        // the scan to everything.
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("a.py"), "TODO\n").unwrap();

        let options = |extensions| ScanOptions {
            patterns_file: PathBuf::from("patterns.yaml"),
            inline_patterns: vec!["TODO".to_string()],
            output: None,
            extensions,
            inputs: vec![temp_dir.path().to_path_buf()],
            workers: Some(1),
            incremental: false,
            force_full: false,
            clear_cache: false,
            content_hash: false,
            no_cache_content: false,
            format: OutputFormat::Text,
            include_summary: false,
            hidden: false,
            no_ignore: false,
        };

        let err = run_scan(options(vec![".".to_string()]))
            .expect_err("-x . must be rejected, not scan everything");
        assert!(
            err.to_string().contains("no valid extensions"),
            "error must name the empty filter: {err}"
        );

        // A normal filter still works.
        run_scan(options(vec!["py".to_string()])).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_unreadable_subdir_does_not_kill_scan() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("readable.txt"), "contact test@example.com\n").unwrap();
        let locked = dir.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("unreachable.txt"), "test@example.com\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let patterns = vec![Pattern {
            name: "email".into(),
            pattern: r"\b[\w._%+-]+@[\w.-]+\.[\w]{2,}\b".into(),
        }];
        let scanner = Scanner::new(patterns).unwrap();

        let sequential = scanner.scan_directory(dir, &[], false, false);
        let parallel = scanner.scan_directory_parallel(dir, &[], Some(1), false, false);
        let collected = collect_files(&[dir.to_path_buf()], &HashSet::new(), false, false);

        // Restore permissions before asserting so TempDir cleanup succeeds.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        let sequential = sequential.unwrap();
        assert_eq!(sequential.len(), 1);
        assert_eq!(sequential[0].file_path, dir.join("readable.txt"));

        let parallel = parallel.unwrap();
        assert_eq!(parallel.len(), 1);
        assert_eq!(parallel[0].file_path, dir.join("readable.txt"));

        let collected = collected.unwrap();
        assert_eq!(collected, vec![dir.join("readable.txt")]);
    }

    #[cfg(unix)]
    #[test]
    fn test_unreadable_file_does_not_kill_scan() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let scanme = temp_dir.path().join("scanme");
        fs::create_dir(&scanme).unwrap();
        fs::write(scanme.join("ok.txt"), "hello TODO world\n").unwrap();
        let locked = scanme.join("locked.txt");
        fs::write(&locked, "TODO locked\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let scanner = Scanner::new(vec![Pattern {
            name: "todo".into(),
            pattern: "TODO".into(),
        }])
        .unwrap();

        let sequential = scanner.scan_directory(&scanme, &[], false, false);
        let parallel = scanner.scan_directory_parallel(&scanme, &[], Some(1), false, false);

        // run_scan: the streaming (text) and collected (json) paths must both
        // succeed and report the readable file's match. Output files live
        // outside the scanned tree so they are not scanned themselves.
        let options = |format, output: PathBuf| ScanOptions {
            patterns_file: PathBuf::from("patterns.yaml"),
            inline_patterns: vec!["TODO".to_string()],
            output: Some(output),
            extensions: vec![],
            inputs: vec![scanme.clone()],
            workers: Some(1),
            incremental: false,
            force_full: false,
            clear_cache: false,
            content_hash: false,
            no_cache_content: false,
            format,
            include_summary: false,
            hidden: false,
            no_ignore: false,
        };
        let text_out = temp_dir.path().join("results.txt");
        let streaming = run_scan(options(OutputFormat::Text, text_out.clone()));
        let json_out = temp_dir.path().join("results.json");
        let collected = run_scan(options(OutputFormat::Json, json_out.clone()));

        // Restore permissions before asserting so TempDir cleanup succeeds.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

        let sequential = sequential.unwrap();
        assert_eq!(sequential.len(), 1);
        assert_eq!(sequential[0].file_path, scanme.join("ok.txt"));

        let parallel = parallel.unwrap();
        assert_eq!(parallel.len(), 1);
        assert_eq!(parallel[0].file_path, scanme.join("ok.txt"));

        streaming.expect("streaming scan must succeed despite an unreadable file");
        let text = fs::read_to_string(&text_out).unwrap();
        assert!(text.contains("ok.txt"), "{text}");
        assert!(!text.contains("locked.txt"), "{text}");

        collected.expect("collected scan must succeed despite an unreadable file");
        let json = fs::read_to_string(&json_out).unwrap();
        assert!(json.contains("ok.txt"), "{json}");
        assert!(!json.contains("locked.txt"), "{json}");
    }

    fn aws_key_scanner() -> Scanner {
        Scanner::new(vec![Pattern {
            name: "aws_key".into(),
            pattern: r"AKIA[0-9A-Z]{16}".into(),
        }])
        .unwrap()
    }

    #[test]
    fn test_hidden_env_file_scanned_only_with_hidden_flag() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join(".env"), "AWS_KEY=AKIAABCDEFGHIJKLMNOP\n").unwrap();

        let scanner = aws_key_scanner();

        let default_matches = scanner.scan_directory(dir, &[], false, false).unwrap();
        assert!(
            default_matches.is_empty(),
            "hidden files must be excluded by default"
        );

        let hidden_matches = scanner.scan_directory(dir, &[], true, false).unwrap();
        assert_eq!(hidden_matches.len(), 1);
        assert_eq!(hidden_matches[0].file_path, dir.join(".env"));

        // collect_files (the run_scan path) must agree with the helpers.
        assert!(
            collect_files(&[dir.to_path_buf()], &HashSet::new(), false, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            collect_files(&[dir.to_path_buf()], &HashSet::new(), true, false).unwrap(),
            vec![dir.join(".env")]
        );
    }

    #[test]
    fn test_gitignored_file_scanned_only_with_no_ignore_flag() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        match std::process::Command::new("git")
            .arg("init")
            .current_dir(dir)
            .output()
        {
            Ok(out) if out.status.success() => {}
            _ => {
                eprintln!("git unavailable; skipping gitignored-scan test");
                return;
            }
        }
        fs::write(dir.join(".gitignore"), "secret.txt\n").unwrap();
        fs::write(dir.join("secret.txt"), "key AKIAABCDEFGHIJKLMNOP\n").unwrap();

        let scanner = aws_key_scanner();

        let default_matches = scanner.scan_directory(dir, &[], false, false).unwrap();
        assert!(
            default_matches.is_empty(),
            "gitignored files must be excluded by default"
        );

        let no_ignore_matches = scanner.scan_directory(dir, &[], false, true).unwrap();
        assert_eq!(no_ignore_matches.len(), 1);
        assert_eq!(no_ignore_matches[0].file_path, dir.join("secret.txt"));

        // collect_files (the run_scan path) must agree with the helpers.
        assert!(
            collect_files(&[dir.to_path_buf()], &HashSet::new(), false, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            collect_files(&[dir.to_path_buf()], &HashSet::new(), false, true).unwrap(),
            vec![dir.join("secret.txt")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_build_scan_state_reuses_carried_fingerprints() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("carried.txt");
        fs::write(&file, "cached content").unwrap();

        let fingerprinter = Fingerprinter::new(true); // content-hash mode
        let fingerprint = fingerprinter.fingerprint_file(&file).unwrap();
        let expected_hash = fingerprint.hash.clone();

        let mut carried = HashMap::new();
        carried.insert(file.clone(), fingerprint);

        // Make the file unreadable: fingerprinting it would fail and leave it
        // out of the state, so finding it there proves the carried fingerprint
        // was reused instead of the file being read a second time.
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        let state = build_scan_state(
            std::slice::from_ref(&file),
            carried,
            &[],
            &[],
            &fingerprinter,
            "patterns".to_string(),
            false,
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(state.files[&file].hash, expected_hash);

        // A file without a carried fingerprint is still fingerprinted normally.
        let fresh = temp_dir.path().join("fresh.txt");
        fs::write(&fresh, "new content").unwrap();
        let state = build_scan_state(
            std::slice::from_ref(&fresh),
            HashMap::new(),
            &[],
            &[],
            &fingerprinter,
            "patterns".to_string(),
            false,
        );
        assert_eq!(
            state.files[&fresh].hash,
            fingerprinter.fingerprint_file(&fresh).unwrap().hash
        );
    }

    #[test]
    fn test_build_scan_state_leaves_out_failed_and_unfingerprintable_files() {
        let temp_dir = TempDir::new().unwrap();
        let ok = temp_dir.path().join("ok.txt");
        fs::write(&ok, "scanned").unwrap();
        // Reported as failed by the scan, with a fingerprint carried over
        // from change detection: the failure must win.
        let failed = temp_dir.path().join("failed.txt");
        fs::write(&failed, "never read").unwrap();
        // Gone by the time the state is built: fingerprinting it fails.
        let missing = temp_dir.path().join("missing.txt");

        let fingerprinter = Fingerprinter::new(false);
        let mut carried = HashMap::new();
        carried.insert(
            failed.clone(),
            fingerprinter.fingerprint_file(&failed).unwrap(),
        );

        let state = build_scan_state(
            &[ok.clone(), failed.clone(), missing.clone()],
            carried,
            &[],
            std::slice::from_ref(&failed),
            &fingerprinter,
            "patterns".to_string(),
            false,
        );

        assert!(state.files.contains_key(&ok));
        assert!(
            !state.files.contains_key(&failed),
            "a file that failed to scan must not be cached as scanned"
        );
        assert!(
            !state.files.contains_key(&missing),
            "a file that cannot be fingerprinted must be left out, not abort"
        );
    }

    #[test]
    fn test_no_cache_content_requires_incremental() {
        let temp_dir = TempDir::new().unwrap();

        let err = run_scan(ScanOptions {
            patterns_file: PathBuf::from("patterns.yaml"),
            inline_patterns: vec!["TODO".to_string()],
            output: None,
            extensions: vec![],
            inputs: vec![temp_dir.path().to_path_buf()],
            workers: Some(1),
            incremental: false,
            force_full: false,
            clear_cache: false,
            content_hash: false,
            no_cache_content: true,
            format: OutputFormat::Text,
            include_summary: false,
            hidden: false,
            no_ignore: false,
        })
        .expect_err("--no-cache-content without --incremental must be rejected");
        assert!(
            err.to_string().contains("--no-cache-content")
                && err.to_string().contains("--incremental"),
            "error must name both flags: {err}"
        );
    }

    #[test]
    fn test_no_cache_content_stores_empty_line_content() {
        let matches = vec![
            Match {
                pattern_name: "secret".into(),
                file_path: PathBuf::from("a.txt"),
                line_number: 3,
                line_content: "password = hunter2".into(),
            },
            Match {
                pattern_name: "todo".into(),
                file_path: PathBuf::from("b.txt"),
                line_number: 7,
                line_content: "TODO later".into(),
            },
        ];

        // With the flag, only the match locations are persisted.
        let stored = build_scan_results(&matches, true);
        assert_eq!(stored.len(), 2);
        for cached in stored.values().flatten() {
            assert!(
                cached.line_content.is_empty(),
                "no matched text may be persisted under --no-cache-content"
            );
        }
        assert_eq!(stored[&PathBuf::from("a.txt")][0].pattern_name, "secret");
        assert_eq!(stored[&PathBuf::from("a.txt")][0].line_number, 3);

        // Without the flag the matched text is stored as before.
        let stored = build_scan_results(&matches, false);
        assert_eq!(
            stored[&PathBuf::from("a.txt")][0].line_content,
            "password = hunter2"
        );
    }

    #[test]
    fn test_resolve_cached_matches_reconstructs_empty_content() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        fs::write(&file, "first line\nsecond line\nthird line\n").unwrap();

        let cached = vec![
            CachedMatch {
                pattern_name: "p1".into(),
                line_number: 1,
                line_content: String::new(),
            },
            CachedMatch {
                pattern_name: "p2".into(),
                line_number: 2,
                // Content-bearing entry: used as-is, no re-read needed.
                line_content: "second line".into(),
            },
            CachedMatch {
                pattern_name: "p3".into(),
                line_number: 3,
                line_content: String::new(),
            },
        ];

        let resolved = resolve_cached_matches(&file, &cached).unwrap();
        assert_eq!(resolved.len(), 3);
        assert_eq!(resolved[0].line_content, "first line");
        assert_eq!(resolved[1].line_content, "second line");
        assert_eq!(resolved[2].line_content, "third line");
        assert_eq!(resolved[0].pattern_name, "p1");
        assert_eq!(resolved[2].line_number, 3);
        assert_eq!(resolved[0].file_path, file);
    }

    #[test]
    fn test_resolve_cached_matches_fails_when_line_or_file_is_gone() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("t.txt");
        fs::write(&file, "only line\n").unwrap();

        // A recorded line beyond the end of the file must be an error the
        // caller can fall back on, not a silently empty match.
        let cached = vec![CachedMatch {
            pattern_name: "p".into(),
            line_number: 5,
            line_content: String::new(),
        }];
        let err = resolve_cached_matches(&file, &cached).expect_err("line 5 does not exist");
        assert!(err.to_string().contains("line 5"), "{err}");

        // An unreadable file must be an error too.
        let missing = temp_dir.path().join("missing.txt");
        assert!(resolve_cached_matches(&missing, &cached).is_err());
    }

    #[test]
    fn test_parallel_produces_same_results() {
        let temp_dir = TempDir::new().unwrap();

        // Create test files
        for i in 0..10 {
            let content = format!("test@example.com file {}", i);
            fs::write(temp_dir.path().join(format!("file{}.txt", i)), content).unwrap();
        }

        let patterns = vec![Pattern {
            name: "email".into(),
            pattern: r"\b[\w._%+-]+@[\w.-]+\.[\w]{2,}\b".into(),
        }];

        let scanner = Scanner::new(patterns).unwrap();

        // Sequential scan
        let mut seq_results = scanner
            .scan_directory(temp_dir.path(), &[], false, false)
            .unwrap();
        seq_results.sort_by_key(|m| m.file_path.clone());

        // Parallel scan
        let mut par_results = scanner
            .scan_directory_parallel(temp_dir.path(), &[], None, false, false)
            .unwrap();
        par_results.sort_by_key(|m| m.file_path.clone());

        // Results should be identical (after sorting)
        assert_eq!(seq_results.len(), par_results.len());
        assert_eq!(seq_results.len(), 10);
    }
}
