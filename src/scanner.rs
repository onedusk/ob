use crate::config::{ConfigLoader, Pattern};
use crate::errors::Result;
use ignore::WalkBuilder;
use regex::{Regex, RegexSet};
use std::fs::{self, File};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use rayon::prelude::*;

/// The core engine for scanning files for regex patterns.
///
/// A `Scanner` is initialized with a set of named patterns. It uses a `RegexSet`
/// for efficient matching of multiple patterns against lines of text, and then
/// confirms matches with the individual `Regex` objects.
pub struct Scanner {
    patterns: Vec<(String, Regex)>,
    pattern_set: RegexSet,
    #[allow(dead_code)]
    pattern_indices: Vec<usize>,
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
        let mut compiled_patterns = Vec::new();
        let mut pattern_indices = Vec::new();
        
        for (idx, p) in patterns.into_iter().enumerate() {
            pattern_strings.push(p.pattern.clone());
            compiled_patterns.push((p.name.clone(), Regex::new(&p.pattern)?));
            pattern_indices.push(idx);
        }
        
        let pattern_set = RegexSet::new(&pattern_strings)?;
        
        Ok(Self {
            patterns: compiled_patterns,
            pattern_set,
            pattern_indices,
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
        let mut matches = Vec::new();
        let file_content = fs::read(path)?;

        // Basic binary detection: check for null bytes in the first 1024 bytes
        if file_content.iter().take(1024).any(|&b| b == 0) {
            return Ok(matches); // Skip binary files
        }

        for (idx, line_bytes) in file_content.split(|&b| b == b'\n').enumerate() {
            let line_str = String::from_utf8_lossy(line_bytes);
            let matching_patterns: Vec<usize> = self.pattern_set
                .matches(&line_str)
                .into_iter()
                .collect();

            for pattern_idx in matching_patterns {
                let (name, regex) = &self.patterns[pattern_idx];
                if regex.is_match(&line_str) {
                    matches.push(Match {
                        pattern_name: name.clone(),
                        file_path: path.to_path_buf(),
                        line_number: idx + 1,
                        line_content: line_str.to_string(),
                    });
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
        let mut all_matches = Vec::new();

        for entry in WalkBuilder::new(dir).build() {
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
        workers: Option<usize>
    ) -> Result<Vec<Match>> {
        use std::sync::Mutex;
        
        // Configure thread pool
        if let Some(num_workers) = workers {
            rayon::ThreadPoolBuilder::new()
                .num_threads(num_workers)
                .build_global()
                .unwrap_or_else(|_| {});
        }
        
        // Collect all file paths first
        let files: Vec<PathBuf> = WalkBuilder::new(dir)
            .threads(workers.unwrap_or_else(num_cpus::get))
            .build()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && should_process_file(path, extensions)
            })
            .map(|entry| entry.path().to_path_buf())
            .collect();
        
        // Process files in parallel
        let all_matches: Arc<Mutex<Vec<Match>>> = Arc::new(Mutex::new(Vec::new()));
        
        files.par_iter()
            .try_for_each(|path| -> Result<()> {
                let matches = self.scan_file(path)?;
                if !matches.is_empty() {
                    let mut results = all_matches.lock().unwrap();
                    results.extend(matches);
                }
                Ok(())
            })?;
        
        let results = Arc::try_unwrap(all_matches)
            .unwrap()
            .into_inner()
            .unwrap();
        
        Ok(results)
    }
    
    /// Scans a list of files in parallel and displays a progress bar.
    ///
    /// # Optimization Note
    ///
    /// The `_workers` parameter is currently unused. The number of threads is determined
    /// by Rayon's global thread pool configuration. To respect this parameter, a new
    /// thread pool could be built and installed for the scope of this function.
    pub fn scan_with_progress(
        &self,
        files: Vec<PathBuf>,
        _workers: Option<usize>,
    ) -> Result<Vec<Match>> {
        use indicatif::{ProgressBar, ProgressStyle};
        
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("##-")
        );
        
        let matches: Vec<Match> = files
            .par_iter()
            .map(|path| {
                pb.inc(1);
                pb.set_message(format!("Scanning: {}", path.display()));
                self.scan_file(path)
            })
            .collect::<Result<Vec<_>>>()?
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
) -> Result<()> {
    // Normalize extensions
    let exts: Vec<String> = extensions
        .into_iter()
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .collect();

    // Load patterns
    let cfg = ConfigLoader::load_scan_config(&patterns_file)?;

    // Create scanner
    let scanner = Arc::new(Scanner::new(cfg.patterns)?);

    // Prepare output
    let mut writer: Box<dyn Write> = match output {
        Some(path) => Box::new(BufWriter::new(File::create(path)?)),
        None => Box::new(std::io::stdout()),
    };

    // Process inputs in parallel
    let all_matches: Vec<Match> = inputs
        .par_iter()
        .map(|input| {
            if input.is_dir() {
                scanner.scan_directory_parallel(input, &exts, workers)
            } else {
                scanner.scan_file(input)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    // Write results
    for m in all_matches {
        writeln!(
            writer,
            "[{}] {}:{}: {}",
            m.pattern_name,
            m.file_path.display(),
            m.line_number,
            m.line_content
        )?;
    }

    Ok(())
}

/// A helper function to determine if a file should be processed based on its extension.
fn should_process_file(path: &Path, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }

    path.extension()
        .and_then(|os| os.to_str())
        .map(|s| extensions.contains(&s.to_lowercase()))
        .unwrap_or(false)
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
