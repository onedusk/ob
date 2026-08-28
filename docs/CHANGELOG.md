# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- **Inline scan patterns:** `ob scan -e '<regex>'` (`--regex`, repeatable) scans without a YAML patterns file. When any `-e` is given, the patterns file is skipped entirely and patterns are auto-named `inline_1`, `inline_2`, ...

### Changed

- Corrected `scan` help text to state it uses `./patterns.yaml` instead of the misleading "default patterns" wording, and added `-e` usage examples.

### Fixed

- `scan` now names the missing patterns file in its error (with a hint to use `-e` or `-p`) instead of emitting a bare `Io(NotFound)`.
- `undo` and `clean-backups` now find `.bak` files that are gitignored (e.g. `*.bak` in the target repo's `.gitignore`) or hidden; backup discovery previously applied the standard gitignore/hidden filters and reported "Backups found: 0" while backups existed.
- `replace` no longer overwrites an existing `<file>.bak` on repeated runs; the oldest backup is preserved because it holds the pre-oober original, so `undo` restores the true original instead of an intermediate state. Docs claiming timestamped backups were corrected (backups never had timestamps).
- `replace` now rejects a config whose `replacements` list is a different length than `patterns`. Previously, missing entries were silently padded with `null` — which means "delete the line after each match" — so an innocent config omission destructively deleted unrelated lines. The error names both lengths; to delete lines intentionally, write an explicit `null` entry.
- `replace` no longer converts CRLF files to LF. The `null`-replacement line-deletion path and the empty-line cleanup after block removal rebuilt files with `\n` regardless of the original line endings, rewriting every line of Windows-encoded files; both paths now detect and preserve the file's line ending style, including the trailing newline.
- `rename` no longer overwrites an existing file when two names map to the same target: the colliding rename is skipped with an error naming both paths and both files are kept. Previously one file's content was silently destroyed (nondeterministically, under parallel execution) while the summary reported success.
- `rename` skips files with non-UTF-8 names with a warning instead of panicking (a panic in one worker aborted the entire run mid-rename).
- `rename` rejects a computed target that would leave the file's directory (a replacement containing `/` or `..`) instead of relocating the file outside the tree.
- `rename` now exits non-zero when any rename failed or was skipped due to a collision or an escaping target, and the summary includes an error count.

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
