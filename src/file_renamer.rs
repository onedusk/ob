use crate::errors::{Error, Result};
use ignore::WalkBuilder;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Executes the file renaming process in a given directory.
///
/// This function walks the specified directory, identifies files matching the
/// provided regex pattern, and renames them using the replacement string.
/// Every target is planned up front, then the renames execute serially in two
/// phases (see [`execute_plan`]) so chains, swaps, and case-only renames
/// succeed and a colliding target never destroys data.
///
/// # Arguments
///
/// * `dir` - The directory to process.
/// * `pattern` - The regex pattern to match against filenames.
/// * `replacement` - The replacement string. Can include capture groups like `$1`.
/// * `dry_run` - If `true`, the planned renames are shown without touching any files.
/// * `workers` - Accepted for CLI compatibility; renames are metadata
///   operations and always execute serially so collision checks cannot race.
pub fn run_rename(
    dir: PathBuf,
    pattern: String,
    replacement: String,
    dry_run: bool,
    verbose: bool,
    workers: Option<usize>,
) -> Result<()> {
    let _ = workers;
    let regex = Regex::new(&pattern)?;
    let replacer = FileRenamer::new(regex, replacement);

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

    let mut processed = 0usize;
    let mut renamed = 0usize;
    let mut errors = 0usize;
    let log_changes = verbose || dry_run;

    // Plan every target before any rename runs. Two files mapping to the same
    // target name are caught here deterministically: the first keeps the
    // target, duplicates are rejected. Case-variant duplicates (final.txt vs
    // Final.txt) are byte-distinct and pass this check; on case-insensitive
    // filesystems they are caught by the execution phase's collision check.
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
                    errors += 1;
                } else {
                    planned.push((path, new_path));
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Error renaming file {}: {}", path.display(), e);
                errors += 1;
            }
        }
        processed += 1;
    }

    if dry_run {
        // Report the plan; collisions only the live filesystem can reveal
        // (external occupants, case-variant duplicates) are reported by a
        // real run. No temp files are created.
        for (path, new_path) in &planned {
            renamed += 1;
            println!("Renamed: {} -> {}", path.display(), new_path.display());
        }
    } else {
        execute_plan(&planned, &mut renamed, &mut errors, log_changes);
    }

    println!("\n{}", "-".repeat(50));
    println!("Files scanned: {}", processed);
    println!("Files renamed: {}", renamed);

    if errors > 0 {
        println!("Errors: {}", errors);
        return Err(Error::RenameFailed(errors));
    }

    Ok(())
}

/// Executes the planned renames serially in two phases.
///
/// Phase A renames every planned source to a unique temporary name in its own
/// directory, freeing all source names at once. Phase B renames each
/// temporary file to its final target; because every planned source has
/// already been moved away, a target that still exists at that point is a
/// genuine external occupant (or a case-variant duplicate on a
/// case-insensitive filesystem), so the rename is refused and the file is
/// restored to its original name.
///
/// This makes chained renames (a -> b while b -> c) and swaps (a <-> b)
/// succeed deterministically, and lets case-only renames (File.txt ->
/// file.txt) succeed on case-insensitive filesystems. Renames are metadata
/// operations, so serial execution costs little; parallel execution let
/// sibling workers race each other's existence checks.
fn execute_plan(
    planned: &[(&PathBuf, PathBuf)],
    renamed: &mut usize,
    errors: &mut usize,
    log_changes: bool,
) {
    // Phase A: move each source out of the way.
    let mut staged: Vec<(&Path, &Path, PathBuf)> = Vec::new();
    for (index, (path, new_path)) in planned.iter().enumerate() {
        let temp = temp_path(path, index);
        match fs::rename(path, &temp) {
            Ok(()) => staged.push((path.as_path(), new_path.as_path(), temp)),
            Err(e) => {
                eprintln!("Error renaming file {}: {}", path.display(), Error::Io(e));
                *errors += 1;
            }
        }
    }

    // Phase B: move each temporary file to its final target. Every planned
    // source was moved away in phase A, so a target that exists here is an
    // external occupant or a case-variant duplicate — never the source itself.
    for (path, new_path, temp) in &staged {
        if new_path.exists() {
            eprintln!(
                "Error renaming file {}: {}",
                path.display(),
                Error::RenameCollision(new_path.to_path_buf())
            );
            *errors += 1;
            restore_source(path, temp);
            continue;
        }
        match fs::rename(temp, new_path) {
            Ok(()) => {
                *renamed += 1;
                if log_changes {
                    println!("Renamed: {} -> {}", path.display(), new_path.display());
                }
            }
            Err(e) => {
                eprintln!("Error renaming file {}: {}", path.display(), Error::Io(e));
                *errors += 1;
                restore_source(path, temp);
            }
        }
    }
}

/// Builds a temporary staging name in the file's own directory, avoiding any
/// existing file (e.g. a temp stranded by an interrupted run).
fn temp_path(path: &Path, index: usize) -> PathBuf {
    let mut candidate = path.with_file_name(format!(".oober-rename-tmp-{index}"));
    let mut bump = 0usize;
    while candidate.exists() {
        bump += 1;
        candidate = path.with_file_name(format!(".oober-rename-tmp-{index}-{bump}"));
    }
    candidate
}

/// Rolls a refused rename's temporary file back to its original name.
///
/// If the original name has since been claimed (a sibling rename in a refused
/// chain), the file is left at the temporary path rather than overwriting;
/// no data is lost either way.
fn restore_source(source: &Path, temp: &Path) {
    if source.exists() {
        eprintln!(
            "Warning: could not restore {}: name is now occupied; file left at {}",
            source.display(),
            temp.display()
        );
        return;
    }
    if let Err(e) = fs::rename(temp, source) {
        eprintln!(
            "Warning: could not restore {}: {}; file left at {}",
            source.display(),
            e,
            temp.display()
        );
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn renamer(pattern: &str, replacement: &str) -> FileRenamer {
        FileRenamer::new(Regex::new(pattern).unwrap(), replacement.to_string())
    }

    /// Returns `true` when the filesystem under `dir` treats names
    /// case-insensitively (the default on macOS and Windows).
    fn fs_is_case_insensitive(dir: &Path) -> bool {
        fs::write(dir.join("x.tmp"), "probe").unwrap();
        let insensitive = dir.join("X.tmp").exists();
        fs::remove_file(dir.join("x.tmp")).unwrap();
        insensitive
    }

    /// Lists the file names in `dir`, sorted.
    fn dir_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn test_collision_with_existing_target_is_skipped() {
        // merged.txt never matches the pattern, so it is a genuine external
        // occupant of the target name: the rename must be refused and the
        // source restored, with both contents untouched.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("file_1.txt"), "one").unwrap();
        fs::write(dir.join("merged.txt"), "existing").unwrap();

        let result = run_rename(
            dir.to_path_buf(),
            r"file_\d".to_string(),
            "merged".to_string(),
            false, // dry_run
            false, // verbose
            None,
        );
        assert!(matches!(result, Err(Error::RenameFailed(1))));

        assert_eq!(dir_names(dir), vec!["file_1.txt", "merged.txt"]);
        assert_eq!(fs::read_to_string(dir.join("file_1.txt")).unwrap(), "one");
        assert_eq!(
            fs::read_to_string(dir.join("merged.txt")).unwrap(),
            "existing"
        );
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
    fn test_run_rename_duplicate_target_never_loses_data() {
        // Regression for the audit repro: file_1.txt + file_2.txt -> merged.txt
        // must not silently destroy one of them. The plan phase rejects the
        // duplicate target before any rename runs; under the old parallel
        // execution the per-file exists() check let both files pass before
        // either rename ran, so one file's content was silently destroyed.
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
                None,
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

    #[test]
    fn test_case_variant_duplicate_targets_keep_both_contents() {
        // On a case-insensitive filesystem, final.txt and Final.txt are
        // byte-distinct targets that pass the plan-phase dedup but collide on
        // disk. The old parallel execution renamed both ("Files renamed: 2",
        // exit 0) and the second rename destroyed the first file's content.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        if !fs_is_case_insensitive(dir) {
            eprintln!("skipping: filesystem is case-sensitive");
            return;
        }
        fs::write(dir.join("final_a.txt"), "one").unwrap();
        fs::write(dir.join("Final_b.txt"), "two").unwrap();

        let result = run_rename(
            dir.to_path_buf(),
            r"_[ab]".to_string(),
            String::new(),
            false, // dry_run
            false, // verbose
            None,
        );
        assert!(matches!(result, Err(Error::RenameFailed(1))));

        // One file won its rename; the other was refused and restored. Both
        // contents survive and no temp files remain.
        let names = dir_names(dir);
        assert_eq!(names.len(), 2, "got: {names:?}");
        assert!(
            !names.iter().any(|n| n.starts_with(".oober-rename-tmp-")),
            "got: {names:?}"
        );
        let mut contents: Vec<String> = names
            .iter()
            .map(|n| fs::read_to_string(dir.join(n)).unwrap())
            .collect();
        contents.sort();
        assert_eq!(contents, vec!["one", "two"]);
    }

    #[test]
    fn test_case_only_rename_succeeds() {
        // On a case-insensitive filesystem, File_1.txt -> file_1.txt was
        // refused as a collision with its own inode: exists() answered for
        // the source itself. Moving the source away first makes it succeed.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("File_1.txt"), "one").unwrap();

        run_rename(
            dir.to_path_buf(),
            "File".to_string(),
            "file".to_string(),
            false, // dry_run
            false, // verbose
            None,
        )
        .unwrap();

        assert_eq!(dir_names(dir), vec!["file_1.txt"]);
        assert_eq!(fs::read_to_string(dir.join("file_1.txt")).unwrap(), "one");
    }

    #[test]
    fn test_chain_rename_succeeds_deterministically() {
        // a.txt -> ab.txt while ab.txt -> abb.txt: one rename's target is the
        // other's source. The old live exists() check raced (or, serially,
        // refused a.txt outright) and left the chain half-applied; the
        // two-phase execution must apply both, every time.
        for _ in 0..10 {
            let temp_dir = TempDir::new().unwrap();
            let dir = temp_dir.path();
            fs::write(dir.join("a.txt"), "A").unwrap();
            fs::write(dir.join("ab.txt"), "B").unwrap();

            run_rename(
                dir.to_path_buf(),
                r"^a(b*)\.txt$".to_string(),
                "ab${1}.txt".to_string(),
                false, // dry_run
                false, // verbose
                None,
            )
            .unwrap();

            assert_eq!(dir_names(dir), vec!["ab.txt", "abb.txt"]);
            assert_eq!(fs::read_to_string(dir.join("ab.txt")).unwrap(), "A");
            assert_eq!(fs::read_to_string(dir.join("abb.txt")).unwrap(), "B");
        }
    }

    #[test]
    fn test_swap_rename_exchanges_names() {
        // ab.txt <-> ba.txt is a rename cycle: neither order of live renames
        // could ever satisfy it. Phase A moves both aside, phase B lands both.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("ab.txt"), "AB").unwrap();
        fs::write(dir.join("ba.txt"), "BA").unwrap();

        run_rename(
            dir.to_path_buf(),
            r"^(a|b)(a|b)\.txt$".to_string(),
            "${2}${1}.txt".to_string(),
            false, // dry_run
            false, // verbose
            None,
        )
        .unwrap();

        assert_eq!(dir_names(dir), vec!["ab.txt", "ba.txt"]);
        assert_eq!(fs::read_to_string(dir.join("ab.txt")).unwrap(), "BA");
        assert_eq!(fs::read_to_string(dir.join("ba.txt")).unwrap(), "AB");
    }

    #[test]
    fn test_dry_run_touches_nothing() {
        // Dry-run reports the plan without renaming and without staging temp
        // files, even for a chain the real run would apply.
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        fs::write(dir.join("a.txt"), "A").unwrap();
        fs::write(dir.join("ab.txt"), "B").unwrap();

        run_rename(
            dir.to_path_buf(),
            r"^a(b*)\.txt$".to_string(),
            "ab${1}.txt".to_string(),
            true,  // dry_run
            false, // verbose
            None,
        )
        .unwrap();

        assert_eq!(dir_names(dir), vec!["a.txt", "ab.txt"]);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "A");
        assert_eq!(fs::read_to_string(dir.join("ab.txt")).unwrap(), "B");
    }
}
