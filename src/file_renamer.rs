use crate::errors::{Error, Result};
use crate::util::resolve_workers;
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashSet;
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

    // Plan every target serially before any rename runs. Two files mapping to
    // the same name are caught here deterministically; checking per file from
    // inside the worker pool was a check-then-rename race in which both files
    // could pass the existence check and one overwrote the other.
    let mut planned: Vec<(&PathBuf, PathBuf)> = Vec::new();
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    for path in &all_files {
        match replacer.plan_rename(path) {
            Ok(Some(new_path)) => {
                if !claimed.insert(new_path.clone()) {
                    eprintln!(
                        "Error renaming file {}: {}",
                        path.display(),
                        Error::RenameCollision(new_path)
                    );
                    errors.fetch_add(1, Ordering::Relaxed);
                } else {
                    planned.push((path, new_path));
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error renaming file {}: {}", path.display(), e);
                errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        processed.fetch_add(1, Ordering::Relaxed);
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(resolve_workers(workers))
        .build()?;

    pool.install(|| {
        planned.par_iter().for_each(|(path, new_path)| {
            match replacer.execute_rename(path, new_path, dry_run) {
                Ok(()) => {
                    renamed.fetch_add(1, Ordering::Relaxed);
                    if log_changes {
                        println!("Renamed: {} -> {}", path.display(), new_path.display());
                    }
                }
                Err(e) => {
                    eprintln!("Error renaming file {}: {}", path.display(), e);
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
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

    /// Computes and validates the rename target for a single file.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(new_path))` if the file's name matches the pattern,
    /// and `Ok(None)` if it did not match (or has a non-UTF-8 name and was
    /// skipped). Returns an error if the computed target would land outside
    /// the source file's directory.
    fn plan_rename(&self, path: &Path) -> Result<Option<PathBuf>> {
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
        Ok(Some(new_path))
    }

    /// Performs a single planned rename.
    ///
    /// Duplicate targets among the planned renames are rejected up front by
    /// `run_rename`, so the existence check here guards against a target that
    /// already exists on disk (a file not part of this run). If `dry_run` is
    /// `true`, the check runs but the rename is not performed.
    fn execute_rename(&self, path: &Path, new_path: &Path, dry_run: bool) -> Result<()> {
        if new_path != path && new_path.exists() {
            return Err(Error::RenameCollision(new_path.to_path_buf()));
        }
        if !dry_run {
            fs::rename(path, new_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn renamer(pattern: &str, replacement: &str) -> FileRenamer {
        FileRenamer::new(Regex::new(pattern).unwrap(), replacement.to_string())
    }

    /// Serial plan-then-execute helper mirroring the old single-call API.
    fn rename_file(r: &FileRenamer, path: &Path, dry_run: bool) -> Result<Option<PathBuf>> {
        match r.plan_rename(path)? {
            Some(new_path) => {
                r.execute_rename(path, &new_path, dry_run)?;
                Ok(Some(new_path))
            }
            None => Ok(None),
        }
    }

    #[test]
    fn test_collision_with_existing_target_is_skipped() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();
        fs::write(dir.join("merged.txt"), "existing").unwrap();

        let r = renamer(r"file_\d", "merged");
        let err = rename_file(&r, &dir.join("file_1.txt"), false).unwrap_err();
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
        let first = rename_file(&r, &dir.join("file_1.txt"), false).unwrap();
        assert_eq!(first, Some(dir.join("merged.txt")));
        let second = rename_file(&r, &dir.join("file_2.txt"), false);
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
        let err = r.plan_rename(&dir.join("file_1.txt")).unwrap_err();
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
        let err = r.plan_rename(&dir.join("file_1.txt")).unwrap_err();
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
        assert!(matches!(r.plan_rename(path), Ok(None)));
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

    #[test]
    fn test_run_rename_parallel_collision_never_loses_data() {
        // Regression for the TOCTOU race: with default (multi-threaded)
        // workers, the old per-file exists() check let both files pass before
        // either rename ran, so one file's content was silently destroyed in
        // most runs. The plan phase must catch the duplicate target no matter
        // how many workers execute the renames.
        for _ in 0..10 {
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
                None,  // default worker count
            );
            assert!(matches!(result, Err(Error::RenameFailed(1))));

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
}
