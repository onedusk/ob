# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- **Inline scan patterns:** `ob scan -e '<regex>'` (`--regex`, repeatable) scans without a YAML patterns file. When any `-e` is given, the patterns file is skipped entirely and patterns are auto-named `inline_1`, `inline_2`, ...
- **`--hidden` / `--no-ignore` scan flags** (ripgrep-style): by default `scan` skips hidden files/directories and anything matched by `.gitignore`/`.ignore` files — which silently hides the most likely secret locations (`.env`, `.github/workflows/`, gitignored config). `--hidden` includes hidden files and `--no-ignore` stops honoring ignore files; the scan help now documents the default exclusions.

### Changed

- `scan --format` now validates its value at parse time: an unknown format (e.g. `-f jsonn`) is a hard CLI error listing the accepted values (`text`, `json`, `csv`, `sarif`, `html`). Previously any unrecognized value silently fell back to `text` output.
- `scan --summary` combined with a non-text `--format` is now an error (`--summary is only supported with --format text`). Previously the flag was silently ignored for `json`, `csv`, `sarif`, and `html` output.
- Corrected `scan` help text to state it uses `./patterns.yaml` instead of the misleading "default patterns" wording, and added `-e` usage examples.
- All docs now agree on `null` replacement semantics: a `null` entry deletes the line *after* each match, and the matching line itself is kept (unchanged behavior). The CLI help previously claimed "delete line", the README "delete the matched line", and the config docs "no replacement" — three contradictory descriptions of one behavior.

### Performance

- Incremental change detection now compares file metadata first and computes the content hash only when sizes match: with `--incremental --content-hash`, a file whose size changed is no longer read and hashed just to be declared changed.
- Incremental scans now carry the fingerprints computed during change detection into state saving instead of re-fingerprinting every file after scanning. A warm `--incremental --content-hash` run previously read and hashed every unchanged file twice; each file is now fingerprinted at most once per run. Cached results and scan output are unchanged.
- `replace` now counts matches during the replacement pass itself instead of running a separate `find_iter().count()` pass first, halving the regex work per pattern and per block on every file. Reported change counts and replacement output are unchanged.

### Fixed

- `replace` now gives explicitly-passed `-x`/`--ext` and `-e`/`--exclude` CLI flags precedence over the config file's `extensions`/`exclude` lists. Previously the config values silently overrode the CLI flags — inverted relative to the usual CLI > config convention — so e.g. `-x py` with a config declaring `extensions: [js]` still processed only `.js` files. Config values now apply only when the corresponding flag is not given.
- `scan` now names the missing patterns file in its error (with a hint to use `-e` or `-p`) instead of emitting a bare `Io(NotFound)`.
- `undo` and `clean-backups` now find `.bak` files that are gitignored (e.g. `*.bak` in the target repo's `.gitignore`) or hidden; backup discovery previously applied the standard gitignore/hidden filters and reported "Backups found: 0" while backups existed.
- `replace` no longer overwrites an existing `<file>.bak` on repeated runs; the oldest backup is preserved because it holds the pre-oober original, so `undo` restores the true original instead of an intermediate state. Docs claiming timestamped backups were corrected (backups never had timestamps).
- `replace` now rejects a config whose `replacements` list is a different length than `patterns`. Previously, missing entries were silently padded with `null` — which means "delete the line after each match" — so an innocent config omission destructively deleted unrelated lines. The error names both lengths; to delete lines intentionally, write an explicit `null` entry.
- `replace` no longer converts CRLF files to LF. The `null`-replacement line-deletion path and the empty-line cleanup after block removal rebuilt files with `\n` regardless of the original line endings, rewriting every line of Windows-encoded files; both paths now detect and preserve the file's line ending style, including the trailing newline.
- `rename` no longer overwrites an existing file when two names map to the same target: the colliding rename is skipped with an error naming both paths and both files are kept. Previously one file's content was silently destroyed (nondeterministically, under parallel execution) while the summary reported success.
- `rename` skips files with non-UTF-8 names with a warning instead of panicking (a panic in one worker aborted the entire run mid-rename).
- `rename` rejects a computed target that would leave the file's directory (a replacement containing `/` or `..`) instead of relocating the file outside the tree.
- `rename` now exits non-zero when any rename failed or was skipped due to a collision or an escaping target, and the summary includes an error count.
- `replace` now exits non-zero when any file fails to process (`replace finished with N file(s) failed`), and the summary includes an error count. Previously per-file errors were printed but swallowed, so the process exited 0 even if every file failed.
- `scan` no longer aborts on the first unreadable path: per-entry walk errors (e.g. a permission-denied subdirectory) now print a warning to stderr and the scan continues with the remaining files. Previously one unreadable directory killed the entire scan (and the parallel walk dropped such errors silently); scan exits non-zero only on fatal errors such as bad patterns or an unwritable output file.
- `replace` now skips binary files, using the same heuristic as `scan` (a null byte in the first 1KB). Previously it tried to read every file as UTF-8 text, so a directory containing images or other binaries produced a per-file error spew — and, combined with the non-zero-exit fix, a spurious failure exit. A skipped binary is not counted as an error.
- All documented preset names now match what the CLI accepts: help text and the no-arg banners showed PascalCase names (`--preset CleanDebug`) that clap rejects; every example now uses the kebab-case form (`--preset clean-debug`), and the replace help lists all seven presets.
- The `trim-whitespace` and `remove-empty-comments` presets now work: their `^`/`$`-anchored patterns were compiled without `(?m)`, and since `replace` applies patterns to the whole file content, the anchors only matched at file boundaries — `trim-whitespace` was a no-op. The presets now use `(?m)` per-line anchors; `remove-empty-comments` also drops a knowingly-broken block entry. The `replace` help now documents that patterns match whole-file content and that per-line `^`/`$` anchors need `(?m)`.

### Security

- CSV output now guards against spreadsheet formula injection: a matched line whose (trimmed) content starts with `=`, `+`, `-`, or `@` gets a single-quote prefix in the `Content` column, so opening the CSV in Excel, Google Sheets, or LibreOffice renders it as text instead of executing it as a formula. Other output formats are unchanged.
- Config file resolution no longer searches the executable's directory, its parent and grandparent, or `~/.uber_scanner` — a config planted next to the binary (or left over in `target/`) can no longer be picked up silently when the given path has a typo. The search is now: the path as given, then the working directory, then `$XDG_CONFIG_HOME/oober` (default `~/.config/oober`); an absolute path is only tried as given, and the not-found error lists exactly the searched locations. `scan -p` now uses the same resolution (it previously only tried the path as given) and reports the resolved patterns file on stderr, matching `replace`, which already printed its resolved config.
- The incremental-scan cache (`$XDG_CACHE_HOME/oober/`, default `~/.cache/oober/`) is now owner-only on Unix: the cache directory is created (and, if pre-existing, tightened) to `0700`, and cache files are written with `0600` permissions. The cache stores the matched line contents of previous scans in plaintext — scanning for secrets with `--incremental` copies those matches into it — and was previously world-readable by default. The README now documents the cache location and its contents.

### Internal

- Added a tuned `[profile.release]` (`lto = true`, `codegen-units = 1`, `strip = true`) for a faster, smaller release binary.
- Removed unused dependencies `anyhow`, `async-trait`, `futures`, and `walkdir` (along with the never-constructed `Error::WalkDir` variant), cutting clean-build and link time.
- `run_scan` and `run_replace` now take a `ScanOptions` / `ReplaceOptions` struct instead of 14 and 11 positional arguments (mostly same-typed, so prone to transposition); the `#[allow(clippy::too_many_arguments)]` escapes are gone, and `ProcessOptions` derives `Clone, Copy` instead of a hand-written `Clone` impl. CLI behavior is unchanged.
- Deleted dead code: the unused `Formatter` trait, `PatternManager`'s never-called `compile` regex cache (its `load_preset` survives as a free function in `patterns`), the never-called `Scanner::scan_with_progress` (dropping the `indicatif` dependency it existed for), the never-constructed `Error::Processing`/`Error::Utf8` variants, and `BlockPattern`'s unread `start`/`end` fields (compiled blocks are now plain `Regex` values).
- Consolidated duplicated helpers into a shared `util` module: `should_process_file` (the replace path now uses the scanner's O(1) `HashSet` lookup and gains its extension normalization — trim, leading-dot strip, lowercase, empty entries dropped), `normalize_extensions` (previously duplicated twice within the scanner), and worker-count resolution (previously copy-pasted across scan, replace, and rename).

---

## [0.1.3] - 2025-09-26

### Added

- Global installation via `cargo install --path .`
- Command alias `ob` for easier access
- Enhanced CLI help messages with practical examples
- Detailed command-specific help with usage examples
- Quick examples in main help output
- Configuration file format examples in help text
- Available presets listed in replace command help
- `rustdoc` support for generating documentation

### Changed

- Improved main CLI description to highlight key features
- Updated README.md to use `ob` alias in all examples
- Updated CLAUDE.md documentation to use `ob` alias
- Better formatting for help messages with bullet points and structure
- Completed documentation in files for `rustdoc`
- Used `rustdoc` to generate the documentation

### Fixed

- Shell alias configuration for zsh users
- Help message quote escaping for proper display

---

## [0.1.2] - 2025-09-25

### Added

- **File Renaming:** Added a new `rename` command to atomically rename files at scale using regex patterns.

---

## [0.1.0] - 2025-09-23

### Initial Release

- SIMD-powered regex pattern matching
- Parallel file processing with Rayon
- Multiple operation modes:
  - `scan`: Find patterns across codebases
  - `replace`: Replace/remove patterns with backup support
  - `undo`: Restore files from backups
  - `clean-backups`: Remove backup files
  - `rename`: Batch rename files using regex
- YAML configuration for patterns and replacements
- Gitignore-aware file traversal
- Automatic backup creation before modifications
- Smart empty line cleanup after block removal
- Support for multi-line block removal
- Incremental scanning with cache support
- Multiple output formats (text, JSON, CSV, SARIF, HTML)
- Dry-run mode for previewing changes
- Built-in presets for common tasks:
  - RemoveCopyright
  - CleanDebug
  - RemoveTodos
  - TrimWhitespace
  - RemoveEmptyComments
  - TabsToSpaces
  - SpacesToTabs

---
