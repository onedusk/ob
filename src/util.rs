//! Small helpers shared across the scan, replace, and rename paths.

use crate::errors::Result;
use std::collections::HashSet;
use std::path::Path;

/// Determines if a file should be processed based on its extension.
///
/// An empty extension set means "process every file". Extensions are compared
/// case-insensitively against the normalized set (see [`normalize_extensions`]).
pub fn should_process_file(path: &Path, extensions: &HashSet<String>) -> bool {
    if extensions.is_empty() {
        return true;
    }

    path.extension()
        .and_then(|os| os.to_str())
        .map(|s| extensions.contains(&s.to_lowercase()))
        .unwrap_or(false)
}

/// Returns `true` if `path` is a `.bak` backup file.
///
/// `replace` and `rename` must never treat backups as ordinary input:
/// rewriting or renaming a `.bak` file breaks `undo`'s guarantee that the
/// oldest backup holds the pre-oober original.
pub fn is_backup_file(path: &Path) -> bool {
    path.extension().and_then(|os| os.to_str()) == Some("bak")
}

/// Normalizes user-supplied extensions (trim whitespace, strip a leading dot,
/// lowercase, drop empty entries) into a set for O(1) lookups.
pub fn normalize_extensions(extensions: &[String]) -> HashSet<String> {
    extensions
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// Normalizes user-supplied extensions like [`normalize_extensions`], but
/// rejects a filter whose entries were provided and all normalize away
/// (e.g. `-x .`): an empty set means "process every file", so silently
/// dropping every entry would flip an explicit filter into processing
/// everything.
pub fn require_valid_extensions(extensions: &[String]) -> Result<HashSet<String>> {
    let normalized = normalize_extensions(extensions);
    if normalized.is_empty() && !extensions.is_empty() {
        return Err("no valid extensions in -x/--ext".into());
    }
    Ok(normalized)
}

/// Resolves the worker-thread count: an explicit request wins, otherwise the
/// number of logical CPU cores (falling back to 1 when that is unavailable).
pub fn resolve_workers(workers: Option<usize>) -> usize {
    workers.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_normalize_extensions_trims_dots_case_and_empties() {
        let exts = normalize_extensions(&strings(&[" .JS ", "py", ".Rs", "", " ", "."]));
        assert_eq!(exts, HashSet::from(["js".into(), "py".into(), "rs".into()]));
    }

    #[test]
    fn test_require_valid_extensions_rejects_all_invalid_filters() {
        // Entries provided but none survive normalization: "-x ." must be a
        // config error, not an empty set (which means "process every file").
        for filter in [&["."][..], &["", " "], &[".", ""]] {
            let err = require_valid_extensions(&strings(filter)).unwrap_err();
            assert!(
                err.to_string().contains("no valid extensions"),
                "filter {filter:?} must be rejected: {err}"
            );
        }

        // No entries at all still means "no filter".
        assert_eq!(require_valid_extensions(&[]).unwrap(), HashSet::new());

        // A normal filter still normalizes as before.
        assert_eq!(
            require_valid_extensions(&strings(&["py", "."])).unwrap(),
            HashSet::from(["py".into()])
        );
    }

    #[test]
    fn test_should_process_file_empty_set_accepts_everything() {
        let exts = HashSet::new();
        assert!(should_process_file(Path::new("a.rs"), &exts));
        assert!(should_process_file(Path::new("no_extension"), &exts));
    }

    #[test]
    fn test_should_process_file_matches_case_insensitively() {
        let exts = normalize_extensions(&strings(&["js"]));
        assert!(should_process_file(Path::new("a.js"), &exts));
        assert!(should_process_file(Path::new("a.JS"), &exts));
        assert!(!should_process_file(Path::new("a.py"), &exts));
        assert!(!should_process_file(&PathBuf::from("no_extension"), &exts));
    }

    #[test]
    fn test_is_backup_file_matches_final_bak_extension_only() {
        assert!(is_backup_file(Path::new("a.txt.bak")));
        assert!(is_backup_file(Path::new("a.txt.bak.bak")));
        assert!(!is_backup_file(Path::new("a.txt")));
        assert!(!is_backup_file(Path::new("a.bak.txt")));
        assert!(!is_backup_file(Path::new("bak")));
    }

    #[test]
    fn test_resolve_workers_explicit_wins_default_is_positive() {
        assert_eq!(resolve_workers(Some(3)), 3);
        assert!(resolve_workers(None) >= 1);
    }
}
