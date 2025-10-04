use crate::errors::Result;
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Executes the file renaming process in a given directory.
///
/// This function walks the specified directory, identifies files matching the
/// provided regex pattern, and renames them using the replacement string. The
/// operation is parallelized using Rayon for performance.
///
/// # Arguments
///
/// * `dir` - The directory to process.
/// * `pattern` - The regex pattern to match against filenames.
/// * `replacement` - The replacement string. Can include capture groups like `$1`.
/// * `dry_run` - If `true`, a preview of changes is shown without actually renaming files.
/// * `workers` - The number of parallel worker threads. If `None`, it defaults to the
///   number of logical CPU cores.
pub fn run_rename(
    dir: PathBuf,
    pattern: String,
    replacement: String,
    dry_run: bool,
    workers: Option<usize>,
) -> Result<()> {
    let regex = Regex::new(&pattern)?;
    let replacer = Arc::new(FileRenamer::new(regex, replacement));

    let mut all_files = Vec::new();
    let mut walker = WalkBuilder::new(&dir);
    walker.standard_filters(true);

    for entry in walker.build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            all_files.push(path.to_path_buf());
        }
    }

    // TODO: For better performance, consider using `Arc<AtomicUsize>` instead of `Mutex`
    // for these simple counters to avoid lock contention in the parallel loop.
    let stats = Arc::new(Mutex::new((0, 0))); // (processed, renamed)

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        }))
        .build()?;

    pool.install(|| {
        all_files.par_iter().for_each(|path| {
            match replacer.rename_file(path, dry_run) {
                Ok(renamed) => {
                    if renamed {
                        let mut s = stats.lock().unwrap();
                        s.1 += 1;
                        println!("Renamed: {} -> {}", path.display(), replacer.get_new_path(path).display());
                    }
                }
                Err(e) => {
                    eprintln!("Error renaming file {}: {}", path.display(), e);
                }
            }
            let mut s = stats.lock().unwrap();
            s.0 += 1;
        });
    });

    let final_stats = stats.lock().unwrap();
    println!("
{}", "-".repeat(50));
    println!("Files scanned: {}", final_stats.0);
    println!("Files renamed: {}", final_stats.1);

    Ok(())
}

/// A helper struct for renaming files based on a regex pattern.
struct FileRenamer {
    regex: Regex,
    replacement: String,
}

impl FileRenamer {
    /// Creates a new `FileRenamer`.
    fn new(regex: Regex, replacement: String) -> Self {
        Self { regex, replacement }
    }

    /// Computes the new path for a file based on the renaming rule.
    ///
    /// # Optimization Note
    ///
    /// This function currently uses `unwrap()` which can panic if a filename is
    /// not valid UTF-8. A more robust implementation would handle this case
    /// gracefully, for example by skipping the file and logging a warning.
    fn get_new_path(&self, path: &Path) -> PathBuf {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        let new_file_name = self.regex.replace_all(file_name, self.replacement.as_str());
        path.with_file_name(new_file_name.into_owned())
    }

    /// Renames a single file if its name matches the pattern.
    ///
    /// If `dry_run` is `true`, it checks if the file would be renamed but doesn't
    /// perform the operation.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the file was (or would be) renamed, and `Ok(false)` otherwise.
    fn rename_file(&self, path: &Path, dry_run: bool) -> Result<bool> {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        if self.regex.is_match(file_name) {
            let new_path = self.get_new_path(path);
            if !dry_run {
                fs::rename(path, &new_path)?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}