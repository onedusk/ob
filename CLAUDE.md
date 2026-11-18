# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

oober (aliased `ob`) is a high-performance pattern scanning and replacement tool built entirely in Rust. It combines ultra-fast regex-based file scanning with powerful pattern replacement capabilities, designed with reliability, speed, and efficiency as top priorities.

Key features:
1. **Scan Mode**: Find patterns across codebases using regex
2. **Replace Mode**: Replace/remove patterns with backup support
3. **Undo Mode**: Restore files from backups
4. **Clean Backups Mode**: Remove backup files without restoring
5. **Parallel Processing**: Uses Rayon for multi-threaded file processing
6. **Respects .gitignore**: Automatically skips ignored files

## Technology Stack

- **Language**: Rust (2024 edition)
- **Pattern Engine**: SIMD-powered regex with YAML configuration
- **File Traversal**: `ignore` crate (gitignore-aware walking; the walk itself is serial, file processing is parallel)
- **CLI Framework**: `clap` v4.5+ with derive macros
- **Parallel Processing**: `rayon` for multi-threading
- **Error Handling**: `thiserror` for custom error types
- **Dependencies**: `tempfile` (atomic writes), `chrono` (scan report timestamps), `serde_yaml_ng` (YAML parsing)

## Common Commands

### Building the Project

```bash
# Install via cargo (recommended)
cargo install --path .

# The binary will be available as 'oober' and aliased as 'ob'
echo "alias ob='oober'" >> ~/.zshrc
source ~/.zshrc

# Build release version (optimized)
cargo build --release

# Development build
cargo build

# Run directly with cargo
cargo run -- scan -p patterns.yaml /path/to/scan
```

### Scanning Files

```bash
# Scan with default patterns.yaml (using alias)
ob scan /path/to/scan

# Scan with custom patterns and output to file
ob scan -p scan_patterns.yaml -o results.txt /path/to/scan

# Scan only specific file types
ob scan -p scan_patterns.yaml -x js,ts,py /path/to/scan
```

### Replacing Patterns

```bash
# Replace using YAML config
ob replace --dir /path/to/process --config emojis.yaml

# Single pattern replacement
ob replace --dir /path/to/process --pattern "shadow-sm" --replacement "shadow-inner"

# Dry-run mode (preview changes)
ob replace --dir /path/to/process --config replace_config.yaml --dry-run

# Clean up oober's backup files
ob clean-backups --dir /path/to/process

# Replace with specific extensions
ob replace --dir /path --config replace_config.yaml -x js,ts
```

### Undo Changes

```bash
# Restore files from the backups oober created
ob undo --dir /path

# Preview what would be restored
ob undo --dir /path --dry-run

# Keep backup files after restore
ob undo --dir /path --keep-backups

# Also restore .bak files oober has no record of creating
ob undo --dir /path --all-bak
```

### Clean Up Backup Files

```bash
# Preview backup files without removing
ob clean-backups --dir /path/to/process --dry-run

# Remove oober's backup files
ob clean-backups --dir /path/to/process

# Also remove .bak files oober has no record of creating
ob clean-backups --dir /path/to/process --all-bak
```

## Architecture

### Module Structure

```
src/
 main.rs             # Entry point, command dispatching
 lib.rs              # Module exports and public API
 cli.rs              # Command-line argument parsing with clap
 scanner.rs          # Pattern scanning implementation
 replacer.rs         # Pattern replacement, undo, and backup management
 backup_records.rs   # Records of the .bak files replace created
 file_renamer.rs     # Batch file renaming with regex patterns
 patterns.rs         # Pattern structure definitions and built-in presets
 config.rs           # YAML configuration parsing and config file resolution
 output_formatter.rs # Scan output formats (text, JSON, CSV, SARIF, HTML)
 state_manager.rs    # Incremental-scan cache persistence
 fingerprint.rs      # File change detection (metadata / content hash)
 util.rs             # Shared helpers (extension filtering, worker resolution)
 errors.rs           # Custom error types with thiserror
```

### Command Structure

The tool uses a subcommand architecture via `cli::Commands` enum:
- `scan`: Find patterns in files (scanner::run_scan)
- `replace`: Replace/remove patterns with optional backup (replacer::run_replace)
- `undo`: Restore files from backups (replacer::run_undo)
- `clean-backups`: Remove backup files without restoring (replacer::run_clean_backups)
- `rename`: Batch rename files using regex patterns (file_renamer::run_rename)

### Core Components

1. **Pattern Engine**: Uses Rust's `regex` crate with SIMD optimizations
2. **File Walker**: `ignore` crate provides gitignore-aware traversal
3. **Parallel Processing**: `rayon` enables multi-threaded file processing with configurable workers
4. **Atomic Writes**: `tempfile` ensures safe file modifications via rename operations
5. **Backup System**: Creates `.bak` files before modifications; an existing `.bak` is never overwritten: `replace` keeps it when it is oober's own backup and the file is unchanged since oober last wrote it, and refuses the file otherwise

### Pattern Configuration

**Scan patterns** (`patterns.yaml`):
```yaml
patterns:
  - name: pattern_identifier
    pattern: 'regex_pattern'
```

**Replace configuration** (`replace_config.yaml`):
```yaml
patterns:
  - "TODO"
  - "console\\.log"
  - "print\\(.*\\)"

replacements:
  - "[TODO]"
  - "// console.log"
  - null  # null means delete the line after each match (the matching line is kept)

blocks:  # Multi-line blocks to remove
  - start: "/* DEBUG START */"
    end: "/* DEBUG END */"

extensions: [js, ts, py]  # Optional
exclude: [node_modules, target]  # Optional
```

## Key Files

- `src/main.rs`: Entry point and command dispatching
- `Cargo.toml`: Dependencies and project configuration
- `config/*.yaml`: Example configs and additional pattern collections (`scan` looks for `./patterns.yaml` by default; override with `-p` or use `-e` for inline regexes)

## Development Workflow

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test scanner::tests

# Run with release optimizations
cargo test --release
```

### Performance Profiling

```bash
# Install flamegraph
cargo install flamegraph

# Generate performance profile
cargo flamegraph --release -- scan -p patterns.yaml /large/codebase

# Benchmark with hyperfine
hyperfine './target/release/oober scan /path' 'rg pattern /path'
```

### Adding New Features

1. **Prioritize reliability over features**
   - Ensure parallel safety with Rayon's Send + Sync bounds
   - Add proper error handling with Result<T> returns
   - Test with large codebases and edge cases

2. **Performance Optimization**
   - Use `cargo build --release` for benchmarking
   - Profile with `cargo flamegraph` or `perf`
   - Leverage SIMD regex operations
   - Minimize allocations in hot paths

3. **Testing Patterns**
   - Create test files with various patterns
   - Test with dry-run mode first
   - Verify backup/restore functionality
   - Check edge cases (binary files, symlinks, empty files)

## Design Principles (Priority Order)

1. **Reliability**: Never corrupt or lose data
   - Atomic file writes
   - Automatic backups before changes
   - Comprehensive error handling

2. **Speed**: Maximize performance
   - Parallel file processing
   - SIMD-powered regex
   - Efficient file traversal

3. **Efficiency**: Optimize resource usage
   - Streaming scan I/O (line-by-line)
   - Minimal memory footprint
   - Smart file filtering

4. **Modularity**: Clean architecture
   - Separate scan/replace/undo logic
   - Reusable pattern configurations
   - Extensible command structure

## Important Notes

- The tool respects `.gitignore` files automatically via the `ignore` crate; `scan` skips hidden files and directories by default too, and accepts `--hidden` / `--no-ignore` to include hidden or ignored files
- Binary files are automatically skipped based on content detection
- `replace` never processes symlinks to files: each is skipped with `Skipping symlink: <path>` on stderr, and its target is edited only under its own path (a directory given as input through a symlink is still walked)
- Backups are created as `.bak` files before any modifications; an existing `.bak` is never overwritten: `replace` keeps it when it is oober's own backup and the file is unchanged since oober last wrote it, and refuses the file otherwise (per-file error naming the backup, non-zero exit, also reported by `--dry-run`; `ob undo`, `ob clean-backups` or `--no-backup` gets past it)
- `replace` records every `.bak` it creates under `$XDG_CACHE_HOME/oober/backups/` (one JSON file per backup, holding the backup's and the file's size and mtime). A `.bak` counts as recorded only while it is a regular file with the recorded size and mtime. `undo` and `clean-backups` act only on recorded backups under `--dir` and leave other `*.bak` files alone unless `--all-bak` is given; `undo --dry-run` previews
- Block removal changes only the blank lines directly next to the removed block; blank lines elsewhere in the file are left as they are
- `scan` matches patterns line-by-line, so scan patterns cannot span lines; `replace` applies patterns to the whole file content — use `(?m)` for per-line `^`/`$` anchors (multi-line removals can also use `blocks`)
- Use double backslashes in YAML for regex escapes (e.g., `\\.` for literal dot)
- Config files are parsed strictly: an unknown key in a scan patterns file or a replace config is an error naming the key. A scan patterns file takes an optional `exclusions` list of names to skip below each input directory (no effect with `-e`, which loads no file)
- `scan` streams files with `BufReader`/`BufWriter`; `replace` reads each file into memory
- File modifications use atomic writes via `tempfile::NamedTempFile`
- The incremental-scan cache lives at `$XDG_CACHE_HOME/oober/` (platform cache dir when unset) and stores matched line contents in plaintext — see the README's cache note

## Recent Enhancements

### Blank Lines Around Removed Blocks
Removing a block changes only the block and the blank lines (spaces and tabs only) directly next to it:
- A block that has its lines to itself is removed with its indent and its line break
- When such a block starts the file or follows a blank line, the blank lines directly after it are removed too (a header block leaves no leading blank line; a block between two paragraphs leaves the blank lines that were above it)
- When such a block ends the file, the blank lines directly before it are removed too
- A block that shares its first or last line with other code is cut out of that line; the code and its line break stay
- Everything else, blank lines elsewhere in the file and LF/CRLF line endings included, is copied byte for byte

### Backup Management
The `clean-backups` command provides:
- Preview mode with `--dry-run` to see what would be removed
- Total size calculation of backup files
- Safe removal with error handling
- Detailed reporting of removed files
