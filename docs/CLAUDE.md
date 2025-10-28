# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Documentation Context
- Claude Code Documentation: @docs/claude-code/doc-index.md
- Implementation Patterns: @.claude/context/implementation-patterns.md
- Current Development Focus: @.claude/context/current-focus.md

## Project Overview

Uber Scanner is a high-performance pattern scanning and replacement tool built entirely in Rust. It combines ultra-fast regex-based file scanning with powerful pattern replacement capabilities, designed with reliability, speed, and efficiency as top priorities.

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
- **File Traversal**: `ignore` crate (gitignore-aware parallel walking)
- **CLI Framework**: `clap` v4.5+ with derive macros
- **Parallel Processing**: `rayon` for multi-threading
- **Error Handling**: `thiserror` for custom error types
- **Dependencies**: `tempfile` (atomic writes), `chrono` (timestamps), `serde_yaml_ng` (YAML parsing)

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

# Clean up backup files
ob clean-backups --dir /path/to/process

# Replace with specific extensions
ob replace --dir /path --config replace_config.yaml -x js,ts
```

### Undo Changes

```bash
# Restore files from backups
ob undo --dir /path

# Keep backup files after restore
ob undo --dir /path --keep-backups
```

### Clean Up Backup Files

```bash
# Preview backup files without removing
ob clean-backups --dir /path/to/process --dry-run

# Remove all backup files
ob clean-backups --dir /path/to/process
```

## Architecture

### Module Structure

```
src/
 main.rs        # Entry point, command dispatching
 lib.rs         # Module exports and public API
 cli.rs         # Command-line argument parsing with clap
 scanner.rs     # Pattern scanning implementation
 replacer.rs    # Pattern replacement, undo, and backup management
 patterns.rs    # Pattern structure definitions
 config.rs      # YAML configuration parsing
 errors.rs      # Custom error types with thiserror
```

### Command Structure

The tool uses a subcommand architecture via `cli::Commands` enum:
- `scan`: Find patterns in files (scanner::run_scan)
- `replace`: Replace/remove patterns with optional backup (replacer::run_replace)
- `undo`: Restore files from backups (replacer::run_undo)
- `clean-backups`: Remove backup files without restoring (replacer::run_clean_backups)

### Core Components

1. **Pattern Engine**: Uses Rust's `regex` crate with SIMD optimizations
2. **File Walker**: `ignore` crate provides gitignore-aware traversal
3. **Parallel Processing**: `rayon` enables multi-threaded file processing with configurable workers
4. **Atomic Writes**: `tempfile` ensures safe file modifications via rename operations
5. **Backup System**: Creates timestamped `.bak` files before modifications

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
  - null  # null means delete the next line

blocks:  # Multi-line blocks to remove
  - start: "/* DEBUG START */"
    end: "/* DEBUG END */"

extensions: [js, ts, py]  # Optional
exclude: [node_modules, target]  # Optional
```

## Key Files

- `src/main.rs`: Unified entry point with all functionality
- `Cargo.toml`: Dependencies and project configuration
- `patterns.yaml`: Default scan patterns
- `replace_config.yaml`: Example replacement configuration
- `patterns/*.yaml`: Additional pattern collections

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
   - Stream processing (line-by-line)
   - Minimal memory footprint
   - Smart file filtering

4. **Modularity**: Clean architecture
   - Separate scan/replace/undo logic
   - Reusable pattern configurations
   - Extensible command structure

## Important Notes

- The tool respects `.gitignore` files automatically via the `ignore` crate
- Binary files are automatically skipped based on content detection
- Backups are created as `.bak` files with timestamps before any modifications
- Empty lines left after block removal are automatically cleaned up
- Line-by-line processing means patterns cannot span multiple lines (use multiline blocks instead)
- Use double backslashes in YAML for regex escapes (e.g., `\\.` for literal dot)
- The tool uses `BufReader` and `BufWriter` for efficient I/O on large files
- File modifications use atomic writes via `tempfile::NamedTempFile`

## Recent Enhancements

### Empty Line Cleanup
After removing multi-line blocks, the tool automatically:
- Removes all empty lines at the beginning of files
- Collapses multiple consecutive empty lines to just one
- Preserves single empty lines between content
- Maintains proper file endings (preserves final newline if present)

### Backup Management
The `clean-backups` command provides:
- Preview mode with `--dry-run` to see what would be removed
- Total size calculation of backup files
- Safe removal with error handling
- Detailed reporting of removed files

---

## PROHIBITED

**All of the following are prohibited in project code**:

- mocks where implementations are supposed to be implemented
- simulations where logic is supposed to be
- placeholders where project code should be
- examples where implementations should be
- scripts to fix large amounts of code || any code that isn't directly approved by the developer
- Half-assed task execution
- half-assed planning
- files created and left in the root of the project
  - this includes ANY non-critical files in root of project
- test files in the root of the project
- un-organized files
