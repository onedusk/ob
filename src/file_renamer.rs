use crate::errors::{Error, Result};
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    verbose: bool,
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

    let processed = AtomicUsize::new(0);
    let renamed = AtomicUsize::new(0);
    let errors = AtomicUsize::new(0);
    let log_changes = verbose || dry_run;

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
                Ok(Some(new_path)) => {
                    renamed.fetch_add(1, Ordering::Relaxed);
                    if log_changes {
                        println!("Renamed: {} -> {}", path.display(), new_path.display());
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("Error renaming file {}: {}", path.display(), e);
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            processed.fetch_add(1, Ordering::Relaxed);
        });
    });

    println!("\n{}", "-".repeat(50));
    println!("Files scanned: {}", processed.load(Ordering::Relaxed));
    println!("Files renamed: {}", renamed.load(Ordering::Relaxed));

    let error_count = errors.load(Ordering::Relaxed);
    if error_count > 0 {
        println!("Errors: {}", error_count);
        return Err(Error::RenameFailed(error_count));
    }

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
    fn get_new_path(&self, path: &Path, file_name: &str) -> PathBuf {
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
    /// Returns `Ok(Some(new_path))` if the file was (or would be) renamed, and
    /// `Ok(None)` if it did not match (or has a non-UTF-8 name and was skipped).
    /// Returns an error if the computed target already exists or would land
    /// outside the source file's directory.
    fn rename_file(&self, path: &Path, dry_run: bool) -> Result<Option<PathBuf>> {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            eprintln!(
                "Warning: skipping file with non-UTF-8 name: {}",
                path.display()
            );
            return Ok(None);
        };
        if !self.regex.is_match(file_name) {
            return Ok(None);
        }
        let new_path = self.get_new_path(path, file_name);
        // A replacement containing `/` (or `..`) would relocate the file via
        // with_file_name; the target must stay in the source's directory.
        if new_path.parent() != path.parent() {
            return Err(Error::RenameEscape(new_path));
        }
        // Best-effort collision check: under parallel renames another worker
        // can still create the target between this check and fs::rename, but
        // it catches two files mapping to the same name in the common case.
        if new_path != path && new_path.exists() {
            return Err(Error::RenameCollision(new_path));
        }
        if !dry_run {
            fs::rename(path, &new_path)?;
        }
        Ok(Some(new_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn renamer(pattern: &str, replacement: &str) -> FileRenamer {
        FileRenamer::new(Regex::new(pattern).unwrap(), replacement.to_string())
    }

    #[test]
    fn test_collision_with_existing_target_is_skipped() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();
        fs::write(dir.join("merged.txt"), "existing").unwrap();

        let r = renamer(r"file_\d", "merged");
        let err = r.rename_file(&dir.join("file_1.txt"), false).unwrap_err();
        assert!(matches!(err, Error::RenameCollision(_)), "got: {err}");

        // Both files intact, contents untouched.
        assert_eq!(fs::read_to_string(dir.join("file_1.txt")).unwrap(), "one");
        assert_eq!(
            fs::read_to_string(dir.join("merged.txt")).unwrap(),
            "existing"
        );
    }

    #[test]
    fn test_two_files_mapping_to_one_name_keep_both_contents() {
        // The audit repro: file_1.txt + file_2.txt -> merged.txt must not
        // silently destroy one of them.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();
        fs::write(dir.join("file_2.txt"), "two").unwrap();

        let r = renamer(r"file_\d", "merged");
        let first = r.rename_file(&dir.join("file_1.txt"), false).unwrap();
        assert_eq!(first, Some(dir.join("merged.txt")));
        let second = r.rename_file(&dir.join("file_2.txt"), false);
        assert!(matches!(second, Err(Error::RenameCollision(_))));

        assert_eq!(fs::read_to_string(dir.join("merged.txt")).unwrap(), "one");
        assert_eq!(fs::read_to_string(dir.join("file_2.txt")).unwrap(), "two");
    }

    #[test]
    fn test_escape_via_parent_dir_is_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();

        let r = renamer(r"file_\d", "../evil");
        let err = r.rename_file(&dir.join("file_1.txt"), false).unwrap_err();
        assert!(matches!(err, Error::RenameEscape(_)), "got: {err}");
        assert!(dir.join("file_1.txt").exists());
        assert!(!dir.parent().unwrap().join("evil.txt").exists());
    }

    #[test]
    fn test_escape_via_subdirectory_is_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();

        let r = renamer(r"file_\d", "sub/moved");
        let err = r.rename_file(&dir.join("file_1.txt"), false).unwrap_err();
        assert!(matches!(err, Error::RenameEscape(_)), "got: {err}");
        assert!(dir.join("file_1.txt").exists());
        assert!(!dir.join("sub").join("moved.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_non_utf8_filename_is_skipped_without_panic() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // The path is never touched on disk: the old code panicked on
        // to_str().unwrap() before any filesystem access.
        let path = Path::new(OsStr::from_bytes(b"\xff\xfe.txt"));
        let r = renamer(".", "x");
        assert!(matches!(r.rename_file(path, false), Ok(None)));
    }

    #[test]
    fn test_run_rename_reports_error_count_and_fails() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();
        fs::write(dir.join("file_2.txt"), "two").unwrap();

        let result = run_rename(
            dir.to_path_buf(),
            r"file_\d".to_string(),
            "merged".to_string(),
            false, // dry_run
            false, // verbose
            Some(1),
        );
        assert!(matches!(result, Err(Error::RenameFailed(1))));

        // One file won the rename; the other was skipped with content intact.
        let survivors: Vec<_> = ["file_1.txt", "file_2.txt"]
            .iter()
            .filter(|name| dir.join(name).exists())
            .collect();
        assert_eq!(survivors.len(), 1);
        let mut contents = vec![
            fs::read_to_string(dir.join("merged.txt")).unwrap(),
            fs::read_to_string(dir.join(survivors[0])).unwrap(),
        ];
        contents.sort();
        assert_eq!(contents, vec!["one", "two"]);
    }
}
