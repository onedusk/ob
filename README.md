# Oober

A pattern scanning and replacement tool built in Rust. It combines SIMD regex-based file scanning with pattern replacement capabilities, designed with reliability, speed, and efficiency as top priorities to cover a lot of ground fast.

## Overview

We currently use it internally to update copyright information, find and change words or code preserving syntax, and automate repetitive tasks. It's also a powerful tool for managing large codebases.

## Features

- **Fast scanning** - SIMD-powered regex engine with parallel file processing using Rayon
- **Pattern matching** - Find complex patterns across entire codebases with YAML configuration
- **Safe replacements** - Replace or remove patterns with automatic `.bak` file creation
- **Atomic file renaming** - Atomically rename files at scale.
- **Undo support** - Restore files from backups with a single command
- **Smart cleanup** - Automatically removes empty lines after block deletion and collapses consecutive empty lines
- **Gitignore aware** - Respects `.gitignore` files automatically via the `ignore` crate
- **Directory exclusions** - Configure custom directory exclusions (e.g., node_modules, target) in YAML
- **Backup management** - Clean up backup files when satisfied with changes
- **Atomic writes** - Uses `tempfile` for safe file modifications
- **Line-by-line processing** - Memory-efficient stream processing for large files

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/oober.git
cd oober

# Install via cargo (recommended)
cargo install --path .

# The binary will be installed to ~/.cargo/bin/oober
# You can now use 'oober' or 'ob' from anywhere

# Add alias to your shell config (.zshrc, .bashrc, etc.)
echo "alias ob='oober'" >> ~/.zshrc
source ~/.zshrc

# Or build manually
cargo build --release
# Binary will be at ./target/release/oober
```

## Quick Start

### Scan for patterns

```bash
# Scan with default patterns.yaml (using alias)
ob scan /path/to/scan

# Scan with custom patterns and output to file
ob scan -p scan_patterns.yaml -o results.txt /path/to/scan

# Scan only specific file types
ob scan -p patterns.yaml -x js,ts,py /path/to/scan
```

### Replace patterns

```bash
# Replace using YAML configuration
ob replace --dir /Users/macadelic/dusk-labs/shared/packages/jot --config replace_config.yaml

# Preview changes without applying (dry-run)
ob replace --dir /path --config replace_config.yaml --dry-run

# Single pattern replacement
ob replace --dir /path --pattern "TODO" --replacement "[TODO]"

# Replace with specific extensions
ob replace --dir /path --config replace_config.yaml -x js,ts
```

### Rename files

```bash
# Rename files with a dry-run to preview changes
ob rename --dir /path/to/project --pattern "old_name" --replacement "new_name" --dry-run

# Rename files
ob rename --dir /path/to/project --pattern "old_name" --replacement "new_name"
```

### Manage backups

```bash
# Restore files from backups
ob undo --dir /path

# Keep backup files after restore
ob undo --dir /path --keep-backups

# Remove backup files after verification
ob clean-backups --dir /Users/macadelic/dusk-labs/shared/packages/jot

# Preview backup files before removal (shows total size)
ob clean-backups --dir /path --dry-run
```

## Configuration

### Scan Patterns (`patterns.yaml`)

```yaml
patterns:
  - name: aws_access_key
    pattern: '\bAKIA[0-9A-Z]{16}\b'

  - name: email_address
    pattern: '\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b'

  - name: password_assignment
    pattern: 'password\s*=\s*[^\s;]+'

# Optional: directories to exclude from scanning
exclusions:
  - node_modules
  - target
  - .git
  - dist
  - build
```

### Replace Configuration (`replace_config.yaml`)

```yaml
# Patterns to search for (regex supported)
patterns:
  - "TODO"
  - "console\\.log"
  - "print\\(.*\\)"

# Replacements (use null to delete the matched line)
replacements:
  - "[TODO]"
  - "// console.log"
  - null  # null means delete the matched line

# Multi-line blocks to remove
blocks:
  - start: "/* DEBUG START */"
    end: "/* DEBUG END */"
  - start: "### TEMP CODE START ###"
    end: "### TEMP CODE END ###"

# Optional: file extensions to process
extensions: [js, ts, py, rb]

# Optional: directories to exclude
exclude: [node_modules, target, .git]
```

## Architecture

### Core Components

1. **Pattern Engine**: Uses Rust's `regex` crate with SIMD optimizations
2. **File Walker**: `ignore` crate provides gitignore-aware traversal
3. **Parallel Processing**: `rayon` enables multi-threaded file processing
4. **Atomic Writes**: `tempfile` ensures safe file modifications
5. **Backup System**: Creates `.bak` files before modifications

### Command Structure

The tool uses a subcommand architecture:

- `scan`: Find patterns in files
- `replace`: Replace/remove patterns with optional backup
- `undo`: Restore files from backups
- `clean-backups`: Remove backup files without restoring

## Use Cases

### Remove copyright headers

```yaml
# remove_copyright.yaml
blocks:
  - start: "# Copyright (c) 2024"
    end: "# DEALINGS IN THE SOFTWARE."
```

```bash
ob replace --dir /project --config remove_copyright.yaml
```

### Clean up debug code

```yaml
# cleanup_debug.yaml
patterns:
  - "console\\.log"
  - "debugger;"
  - "print\\(.*debug.*\\)"

replacements:
  - "// console.log"
  - "// debugger;"
  - "# print(debug)"

blocks:
  - start: "// DEBUG START"
    end: "// DEBUG END"

# Exclude common build directories
exclude:
  - node_modules
  - target
  - dist
```

### Security audit with exclusions

```yaml
# secret_patterns.yaml
patterns:
  - name: aws_access_key
    pattern: '\bAKIA[0-9A-Z]{16}\b'
  - name: private_key
    pattern: '-----BEGIN (?:RSA )?PRIVATE KEY-----'

exclusions:
  - node_modules
  - vendor
  - .git
  - target
  - dist
```

```bash
# Scan for potential security issues
ob scan -p patterns/secret_patterns.yaml -o security_audit.txt /project

# Additional pattern collections available
ob scan -p patterns/grammar_patterns.yaml /project  # Grammar checks
ob scan -p patterns/log_patterns.yaml /project      # Log statements
```

## Command Reference

### `scan`

Find patterns in files and directories.

Options:

- `-p, --patterns <FILE>` - Path to YAML patterns file (default: patterns.yaml)
- `-o, --output <FILE>` - Output file (default: stdout)
- `-x, --ext <EXTENSIONS>` - Comma-separated file extensions to include
- `<INPUTS>...` - Files or directories to scan

### `replace`

Replace or remove patterns in files.

Options:

- `-c, --config <FILE>` - YAML configuration file
- `-p, --pattern <PATTERN>` - Single pattern to match
- `-r, --replacement <TEXT>` - Replacement text
- `-d, --dir <PATH>` - Directory to process
- `-x, --ext <EXTENSIONS>` - File extensions to include
- `-e, --exclude <DIRS>` - Directories to exclude
- `--no-backup` - Don't create backup files
- `--dry-run` - Preview changes without applying
- `-w, --workers <N>` - Number of threads to use

### `undo`

Restore files from backups.

Options:

- `-d, --dir <PATH>` - Directory to restore
- `--keep-backups` - Don't delete backup files after restore

### `clean-backups`

Remove backup files without restoring.

Options:

- `-d, --dir <PATH>` - Directory to clean
- `--dry-run` - Preview files to be removed

### `rename`

Rename files in a directory.

Options:

- `-d, --dir <PATH>` - Directory to process
- `-p, --pattern <PATTERN>` - Regex pattern to match filenames
- `-r, --replacement <TEXT>` - Replacement string
- `--dry-run` - Preview changes without applying
- `-w, --workers <N>` - Number of threads to use

## Performance

Oober is designed for maximum performance:

- **Parallel processing** - Uses all available CPU cores via Rayon
- **SIMD regex** - Hardware-accelerated pattern matching
- **Efficient I/O** - Streams files line-by-line to minimize memory usage
- **Smart filtering** - Respects .gitignore and filters by extension early
- **Minimal allocations** - Optimized for hot paths

Benchmark on a large codebase (100k files):

- Scanning: ~2-5 seconds
- Replacing: ~5-10 seconds (with backup creation)

### Performance Tips

- Use `cargo build --release` for production builds
- Profile with `cargo flamegraph` for optimization
- Filter by extensions to reduce file processing

## Safety Features

- **Automatic backups** - Creates `.bak` files before any modification
- **Atomic writes** - Uses temporary files to prevent corruption
- **Dry-run mode** - Preview all changes before applying
- **Binary detection** - Automatically skips binary files
- **Gitignore respect** - Won't modify ignored files
- **Comprehensive error handling** - Graceful failures with clear messages
- **Empty line cleanup** - Intelligently handles whitespace after deletions

## Building from Source

Requirements:

- Rust 2024 edition
- Cargo

Dependencies:

- `regex` - Pattern matching engine
- `rayon` - Parallel processing
- `ignore` - Gitignore-aware file walking
- `clap` - Command line parsing
- `tempfile` - Atomic file operations
- `chrono` - Timestamp handling

```bash
# Development build
cargo build

# Run tests
cargo test

# Build optimized binary
cargo build --release
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

Design principles (in priority order):

1. **Reliability** - Never corrupt or lose data
   - Atomic file writes
   - Automatic backups before changes
   - Comprehensive error handling
2. **Speed** - Maximize performance
   - Parallel file processing
   - SIMD-powered regex
   - Efficient file traversal
3. **Efficiency** - Optimize resource usage
   - Stream processing (line-by-line)
   - Minimal memory footprint
   - Smart file filtering
4. **Modularity** - Clean architecture
   - Separate scan/replace/undo logic
   - Reusable pattern configurations
   - Extensible command structure

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Important Notes

- The tool respects `.gitignore` files automatically
- Binary files are automatically skipped
- Backups are created as `.bak` files before any modifications
- Empty lines left after block removal are automatically cleaned up
- Line-by-line processing means patterns cannot span multiple lines
- Use double backslashes in YAML for regex escapes (e.g., `\\.` for literal dot)

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

## Acknowledgments

Built with:

- [regex](https://github.com/rust-lang/regex) - Rust's regex engine with SIMD support
- [rayon](https://github.com/rayon-rs/rayon) - Data parallelism library
- [ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) - Gitignore parser
- [clap](https://github.com/clap-rs/clap) - Command line parser
- [tempfile](https://github.com/Stebalien/tempfile) - Secure temporary file handling
- [chrono](https://github.com/chronotope/chrono) - Date and time library
