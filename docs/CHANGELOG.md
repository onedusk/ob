# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

## [0.1.2] - 2025-09-25

### Added

- **File Renaming:** Added a new `rename` command to atomically rename files at scale using regex patterns.

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
