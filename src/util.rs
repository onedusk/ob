//! Small helpers shared across the scan, replace, and rename paths.

use crate::errors::Result;
use regex::Regex;
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

/// Checks that every capture-group reference in `replacement` names a group
/// that `regex` defines.
///
/// The replacement is walked by the regex crate's own expansion rule: `$$` is
/// a literal dollar sign; `${name}` is a braced reference; otherwise `$` is
/// followed by the longest run of `[0-9A-Za-z_]`; a `$` followed by none of
/// those stays literal. A reference that parses as a number is a group index,
/// anything else a group name.
///
/// The crate expands a reference to a group that does not exist to nothing,
/// silently dropping text: `$2_$1` reads `$2_` as a group named `2_`. Such a
/// reference is rejected here instead.
pub fn validate_replacement(regex: &Regex, replacement: &str) -> Result<()> {
    let bytes = replacement.as_bytes();
    let mut i = 0;
    while let Some(offset) = bytes[i..].iter().position(|&b| b == b'$') {
        // Step past the `$`.
        i += offset + 1;
        let (name, end) = match bytes.get(i) {
            Some(b'$') => {
                i += 1;
                continue;
            }
            Some(b'{') => match bytes[i + 1..].iter().position(|&b| b == b'}') {
                Some(len) => (&replacement[i + 1..i + 1 + len], i + len + 2),
                // An unclosed brace leaves the `$` literal.
                None => continue,
            },
            _ => {
                let len = bytes[i..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                    .count();
                if len == 0 {
                    continue;
                }
                (&replacement[i..i + len], i + len)
            }
        };

        let defined = match name.parse::<usize>() {
            Ok(index) => index < regex.captures_len(),
            Err(_) => regex.capture_names().flatten().any(|n| n == name),
        };
        if !defined {
            return Err(format!(
                "replacement '{replacement}' references capture group '{name}', which pattern '{}' does not define; write ${{1}} (with braces) to end a group reference before a letter, digit or underscore, and $$ for a literal dollar sign",
                regex.as_str()
            )
            .into());
        }
        i = end;
    }
    Ok(())
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
    fn test_validate_replacement_accepts_defined_references_and_literals() {
        let regex = Regex::new(r"(\d+)_(?P<rest>\w+)").unwrap();
        for ok in [
            "plain text",
            "$0",
            "$1",
            "$2",
            "${1}",
            "${2}_${1}",
            "$rest",
            "${rest}_x",
            "spec_$1",
            // Literal dollars: `$$`, a trailing `$`, a `$` before a
            // non-reference character, and an unclosed brace.
            "$$",
            "$$5",
            "$$rest_x",
            "cost: $",
            "$ 5",
            "$-1",
            "${1",
            "\u{e9}$1\u{e9}",
        ] {
            assert!(
                validate_replacement(&regex, ok).is_ok(),
                "{ok:?} must be accepted"
            );
        }
    }

    #[test]
    fn test_validate_replacement_rejects_undefined_references() {
        let regex = Regex::new(r"(\d+)_(?P<rest>\w+)").unwrap();
        for (bad, name) in [
            // `$2_` and `$1x` are names, not group 2 or 1 plus a suffix.
            ("$2_$1", "2_"),
            ("$1x", "1x"),
            ("$3", "3"),
            ("${3}", "3"),
            ("$missing", "missing"),
            ("${missing}", "missing"),
            ("${}", ""),
            // A literal `$$` does not hide a later bad reference.
            ("$$$5", "5"),
            ("${1}$rest_x", "rest_x"),
        ] {
            let msg = validate_replacement(&regex, bad)
                .expect_err("must be rejected")
                .to_string();
            assert!(msg.contains(&format!("'{name}'")), "{bad:?}: {msg}");
            assert!(
                msg.contains("${1}") && msg.contains("$$"),
                "the error must show the braced form and the escape: {msg}"
            );
        }

        // Group 0 is the whole match; a pattern without groups has nothing else.
        let plain = Regex::new("PRICE").unwrap();
        assert!(validate_replacement(&plain, "$0").is_ok());
        assert!(validate_replacement(&plain, "$5").is_err());
    }

    #[test]
    fn test_validate_replacement_agrees_with_regex_expansion() {
        // The validator and the crate must read a replacement the same way:
        // expanding each accepted form against a match shows every reference
        // resolved and every literal dollar kept.
        let regex = Regex::new(r"(a)(?P<rest>b)").unwrap();
        for (replacement, expanded) in [
            ("${2}_${1}", "b_a"),
            ("$rest$1", "ba"),
            ("$$1", "$1"),
            ("${1", "${1"),
            ("$ $0", "$ ab"),
        ] {
            assert!(validate_replacement(&regex, replacement).is_ok());
            assert_eq!(
                regex.replace("ab", replacement),
                expanded,
                "{replacement:?}"
            );
        }
    }

    #[test]
    fn test_resolve_workers_explicit_wins_default_is_positive() {
        assert_eq!(resolve_workers(Some(3)), 3);
        assert!(resolve_workers(None) >= 1);
    }
}
