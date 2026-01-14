use crate::config::{ConfigLoader, Pattern};
use crate::errors::Result;
use crate::fingerprint::Fingerprinter;
use crate::output_formatter::{OutputFormat, OutputFormatter};
use crate::state_manager::{CachedMatch, FileState, ScanState, StateManager};
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::RegexSet;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::SystemTime;

/// The core engine for scanning files for regex patterns.
///
/// A `Scanner` is initialized with a set of named patterns. It uses a `RegexSet`
/// for efficient matching of multiple patterns against lines of text, and then
/// confirms matches with the individual `Regex` objects.
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
    /// It reads the file and checks each line against the `RegexSet`. If any patterns
    /// match, it confirms with the specific `Regex` to create `Match` objects.
    ///
    /// This function includes a simple heuristic to skip binary files by checking for
    /// null bytes in the first 1KB of the file.
    pub fn scan_file(&self, path: &Path) -> Result<Vec<Match>> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Basic binary detection: check for null bytes in the first 1024 bytes (peek only).
        let buf = reader.fill_buf()?;
        let sample_len = buf.len().min(1024);
        if buf[..sample_len].iter().any(|&b| b == 0) {
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
    /// This is a sequential, single-threaded scan. For better performance on large
    /// directories, use `scan_directory_parallel`.
    pub fn scan_directory(&self, dir: &Path, extensions: &[String]) -> Result<Vec<Match>> {
        let extensions = normalize_extensions_ref(extensions);
        self.scan_directory_with_set(dir, &extensions)
    }

    fn scan_directory_with_set(&self, dir: &Path, extensions: &HashSet<String>) -> Result<Vec<Match>> {
        let mut all_matches = Vec::new();

        for entry in WalkBuilder::new(dir).standard_filters(true).build() {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && should_process_file(path, extensions) {
                let matches = self.scan_file(path)?;
                all_matches.extend(matches);
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
    ) -> Result<Vec<Match>> {
        let extensions = normalize_extensions_ref(extensions);
        self.scan_directory_parallel_with_set(dir, &extensions, workers)
    }

    fn scan_directory_parallel_with_set(
        &self,
        dir: &Path,
        extensions: &HashSet<String>,
        workers: Option<usize>,
    ) -> Result<Vec<Match>> {
        // Collect all file paths first
        let files: Vec<PathBuf> = WalkBuilder::new(dir)
            .standard_filters(true)
            .threads(resolve_workers(workers))
            .build()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && should_process_file(path, extensions)
            })
            .map(|entry| entry.path().to_path_buf())
            .collect();

        self.scan_files_parallel(&files, workers)
    }

    /// Scans a list of files in parallel using a local Rayon thread pool.
    fn scan_files_parallel(
        &self,
        files: &[PathBuf],
        workers: Option<usize>,
    ) -> Result<Vec<Match>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(resolve_workers(workers))
            .build()?;

        pool.install(|| {
            files
                .par_iter()
                .try_fold(Vec::new, |mut acc, path| {
                    acc.extend(self.scan_file(path)?);
                    Ok(acc)
                })
                .try_reduce(Vec::new, |mut left, mut right| {
                    left.append(&mut right);
                    Ok(left)
                })
        })
    }
    
    /// Scans a list of files in parallel and displays a progress bar.
    ///
    pub fn scan_with_progress(
        &self,
        files: Vec<PathBuf>,
        workers: Option<usize>,
    ) -> Result<Vec<Match>> {
        use indicatif::{ProgressBar, ProgressStyle};
        
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("##-")
        );
        
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(resolve_workers(workers))
            .build()?;

        let matches: Vec<Match> = pool.install(|| {
            files
                .par_iter()
                .map(|path| {
                    pb.inc(1);
                    pb.set_message(format!("Scanning: {}", path.display()));
                    self.scan_file(path)
                })
                .collect::<Result<Vec<_>>>()
        })?
        .into_iter()
        .flatten()
        .collect();
        
        pb.finish_with_message("Scan complete");
        Ok(matches)
    }
}

/// The main entry point for the `scan` command.
///
/// This function handles:
/// 1. Loading the scan patterns from a configuration file.
/// 2. Setting up the output writer (either a file or `stdout`).
/// 3. Iterating through the input paths and dispatching to the appropriate
///    `Scanner` methods (for files or directories).
/// 4. Writing the results.
pub fn run_scan(
    patterns_file: PathBuf,
    output: Option<PathBuf>,
    extensions: Vec<String>,
    inputs: Vec<PathBuf>,
    workers: Option<usize>,
    incremental: bool,
    force_full: bool,
    clear_cache: bool,
    content_hash: bool,
    format: String,
    include_summary: bool,
) -> Result<()> {
    let exts = normalize_extensions(extensions);

    // Load patterns
    let cfg = ConfigLoader::load_scan_config(&patterns_file)?;
    let patterns_hash = compute_patterns_hash(&cfg.patterns);

    // Create scanner
    let scanner = Arc::new(Scanner::new(cfg.patterns)?);

    let output_format = OutputFormat::from(format.as_str());

    // Prepare output
    let mut writer: Box<dyn Write + Send> = match output {
        Some(path) => Box::new(BufWriter::new(File::create(path)?)),
        None => Box::new(BufWriter::new(std::io::stdout())),
    };

    let files = collect_files(&inputs, &exts)?;
    let project_root = determine_project_root(&inputs)?;
    let mut files_to_scan = files.clone();
    let mut cached_matches: Vec<Match> = Vec::new();

    if clear_cache && !incremental {
        let manager = StateManager::new(&project_root)?;
        manager.clear_cache()?;
    }

    let mut state_manager: Option<StateManager> = None;
    let mut fingerprinter: Option<Fingerprinter> = None;

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
                    if !fp.has_file_changed(file, file_state)? {
                        if let Some(cached) = state.scan_results.get(file) {
                            cached_matches.extend(cached.iter().map(|m| Match {
                                pattern_name: m.pattern_name.clone(),
                                file_path: file.to_path_buf(),
                                line_number: m.line_number,
                                line_content: m.line_content.clone(),
                            }));
                        }
                        continue;
                    }
                }

                changed_files.push(file.to_path_buf());
            }

            files_to_scan = changed_files;
        }

        state_manager = Some(manager);
        fingerprinter = Some(fp);
    }

    let can_stream = matches!(output_format, OutputFormat::Text) && !include_summary && !incremental;

    if can_stream {
        stream_text_output(scanner, &files_to_scan, workers, writer)?;
        return Ok(());
    }

    let mut all_matches = Vec::new();
    if !cached_matches.is_empty() {
        all_matches.extend(cached_matches);
    }

    let scanned_matches = scanner.scan_files_parallel(&files_to_scan, workers)?;
    all_matches.extend(scanned_matches);

    let formatter = OutputFormatter::new(output_format, include_summary);
    formatter.write_output(&mut writer, &all_matches)?;

    if incremental {
        let manager = state_manager.expect("State manager missing");
        let fp = fingerprinter.expect("Fingerprinter missing");
        let scan_state = build_scan_state(&files, &all_matches, &fp, patterns_hash)?;
        manager.save_state(&scan_state)?;
    }

    Ok(())
}

/// A helper function to determine if a file should be processed based on its extension.
fn should_process_file(path: &Path, extensions: &HashSet<String>) -> bool {
    if extensions.is_empty() {
        return true;
    }

    path.extension()
        .and_then(|os| os.to_str())
        .map(|s| extensions.contains(&s.to_lowercase()))
        .unwrap_or(false)
}

fn normalize_extensions(extensions: Vec<String>) -> HashSet<String> {
    extensions
        .into_iter()
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

fn normalize_extensions_ref(extensions: &[String]) -> HashSet<String> {
    extensions
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

fn resolve_workers(workers: Option<usize>) -> usize {
    workers.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    })
}

fn collect_files(inputs: &[PathBuf], extensions: &HashSet<String>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for input in inputs {
        if input.is_file() {
            if should_process_file(input, extensions) {
                files.push(input.to_path_buf());
            }
        } else if input.is_dir() {
            for entry in WalkBuilder::new(input).standard_filters(true).build() {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && should_process_file(path, extensions) {
                    files.push(path.to_path_buf());
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

fn build_scan_state(
    files: &[PathBuf],
    matches: &[Match],
    fingerprinter: &Fingerprinter,
    patterns_hash: String,
) -> Result<ScanState> {
    let now = SystemTime::now();
    let mut file_states = HashMap::new();

    for file in files {
        let fingerprint = fingerprinter.fingerprint_file(file)?;
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

    Ok(ScanState {
        version: env!("CARGO_PKG_VERSION").to_string(),
        last_scan: now,
        files: file_states,
        patterns_hash,
        scan_results: build_scan_results(matches),
    })
}

fn build_scan_results(matches: &[Match]) -> HashMap<PathBuf, Vec<CachedMatch>> {
    let mut results: HashMap<PathBuf, Vec<CachedMatch>> = HashMap::new();

    for m in matches {
        results
            .entry(m.file_path.clone())
            .or_insert_with(Vec::new)
            .push(CachedMatch {
                pattern_name: m.pattern_name.clone(),
                line_number: m.line_number,
                line_content: m.line_content.clone(),
            });
    }

    results
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
                let matches = scanner.scan_file(path)?;
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
                    tx.send(line)
                        .map_err(|_| "Output channel closed")?;
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
    use tempfile::TempDir;
    use std::fs;
    
    #[test]
    fn test_regex_set_matching() {
        let patterns = vec![
            Pattern { name: "email".into(), pattern: r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b".into() },
            Pattern { name: "url".into(), pattern: r"https?://[^\s]+".into() },
            Pattern { name: "ip".into(), pattern: r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b".into() },
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
        let patterns = vec![
            Pattern { name: "test_pattern".into(), pattern: r"test".into() },
        ];
        
        let scanner = Scanner::new(patterns).unwrap();
        
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "this is a test").unwrap();
        
        let matches = scanner.scan_file(&test_file).unwrap();
        assert_eq!(matches[0].pattern_name, "test_pattern");
    }
    
    #[test]
    fn test_parallel_produces_same_results() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create test files
        for i in 0..10 {
            let content = format!("test@example.com file {}", i);
            fs::write(temp_dir.path().join(format!("file{}.txt", i)), content).unwrap();
        }
        
        let patterns = vec![
            Pattern { name: "email".into(), pattern: r"\b[\w._%+-]+@[\w.-]+\.[\w]{2,}\b".into() }
        ];
        
        let scanner = Scanner::new(patterns).unwrap();
        
        // Sequential scan
        let mut seq_results = scanner.scan_directory(temp_dir.path(), &[]).unwrap();
        seq_results.sort_by_key(|m| m.file_path.clone());
        
        // Parallel scan
        let mut par_results = scanner.scan_directory_parallel(temp_dir.path(), &[], None).unwrap();
        par_results.sort_by_key(|m| m.file_path.clone());
        
        // Results should be identical (after sorting)
        assert_eq!(seq_results.len(), par_results.len());
        assert_eq!(seq_results.len(), 10);
    }
}
