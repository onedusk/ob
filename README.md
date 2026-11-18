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
- **Local block removal** - A removed block takes only its own lines and the blank lines directly next to it; blank lines elsewhere in the file are left alone
- **Gitignore aware** - Respects `.gitignore` files automatically via the `ignore` crate (`scan` offers `--hidden` / `--no-ignore` to opt out)
- **Directory exclusions** - Configure custom directory exclusions (e.g., node_modules, target) in YAML
- **Backup management** - Clean up backup files when satisfied with changes
- **Atomic writes** - Uses `tempfile` for safe file modifications
- **Streaming scans** - `scan` reads files line-by-line for a small memory footprint (`replace` reads each file into memory to apply whole-file patterns)

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

### Shell completions & man page

Both commands write to standard output only; redirect to install:

```bash
# zsh (with ~/.zfunc in your fpath)
ob completions zsh > ~/.zfunc/_oober

# bash
ob completions bash > ~/.local/share/bash-completion/completions/oober

# fish
ob completions fish > ~/.config/fish/completions/oober.fish

# powershell and elvish are also supported

# Man page (preview without installing: ob manpage > oober.1 && man ./oober.1)
ob manpage > /usr/local/share/man/man1/oober.1
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
# Preview changes without applying (dry-run)
ob replace /path --config replace_config.yaml --dry-run

# Single pattern replacement
ob replace /path --pattern "TODO" --replacement "[TODO]"

# Replace across multiple directories, or in a single file
ob replace src/ lib/ --pattern "TODO" --replacement "[TODO]"
ob replace main.py --preset remove-todos

# Replace with specific extensions
ob replace /path --config replace_config.yaml -x js,ts
```

`--dir /path` is still accepted as a legacy alias for a positional input.

### Rename files

```bash
# Rename files with a dry-run to preview changes
ob rename --dir /path/to/project --pattern "old_name" --replacement "new_name" --dry-run

# Rename files
ob rename --dir /path/to/project --pattern "old_name" --replacement "new_name"

# Reorder name parts with capture groups (12_intro.md -> intro_12.md)
ob rename --dir /path/to/project --pattern '^(\d+)_(.+?)(\.[^.]+)?$' --replacement '${2}_${1}${3}'
```

### Manage backups

```bash
# Restore files from the backups oober created
ob undo --dir /path

# Preview what would be restored
ob undo --dir /path --dry-run

# Keep backup files after restore
ob undo --dir /path --keep-backups

# Remove oober's backup files after verification
ob clean-backups --dir /path

# Preview backup files before removal (shows total size)
ob clean-backups --dir /path --dry-run

# Also act on .bak files oober has no record of creating (preview first)
ob undo --dir /path --all-bak --dry-run
ob clean-backups --dir /path --all-bak --dry-run
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

`exclusions` names are matched against the path below each input directory (not against the input's own path), and an excluded directory is not walked, even with `--hidden` or `--no-ignore`. A file named as an input is never excluded. The list comes from the patterns file, so it has no effect with `-e` inline patterns, which load no file.

The only keys are `patterns` (each entry with `name` and `pattern`) and `exclusions`. Any other key is an error that names the key, so a typo cannot be silently ignored. An empty `pattern` string is an error too: an empty regex matches every line.

### Replace Configuration (`replace_config.yaml`)

```yaml
# Patterns to search for (regex supported)
patterns:
  - "TODO"
  - "console\\.log"
  - "print\\(.*\\)"

# Replacements (null deletes the line AFTER each match; the matching line is kept)
replacements:
  - "[TODO]"
  - "// console.log"
  - null  # null means delete the line after each match

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

The only keys are `patterns`, `replacements`, `blocks` (each entry with `start` and `end`), `extensions` and `exclude`. Any other key is an error that names the key, so a typo cannot be silently ignored. An empty string in `patterns` is an error too: an empty regex matches at every position. `patterns` and `replacements` may both be left out of a config that only removes `blocks`.

## Architecture

### Core Components

1. **Pattern Engine**: Uses Rust's `regex` crate with SIMD optimizations
2. **File Walker**: `ignore` crate provides gitignore-aware traversal
3. **Parallel Processing**: `rayon` enables multi-threaded file processing
4. **Atomic Writes**: `tempfile` ensures safe file modifications
5. **Backup System**: Creates `.bak` files before modifications; an existing `.bak` is never overwritten: `replace` keeps it when it is oober's own backup and the file is unchanged since oober last wrote it, and refuses the file otherwise

### Command Structure

The tool uses a subcommand architecture:

- `scan`: Find patterns in files
- `replace`: Replace/remove patterns with optional backup
- `undo`: Restore files from backups
- `clean-backups`: Remove backup files without restoring
- `rename`: Batch rename files using regex patterns
- `completions`: Generate a shell completion script on stdout
- `manpage`: Print the roff man page on stdout

## Use Cases

### Remove copyright headers

```yaml
# remove_copyright.yaml
blocks:
  - start: "# Copyright (c) 2024"
    end: "# DEALINGS IN THE SOFTWARE."
```

```bash
ob replace /project --config remove_copyright.yaml
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
# Scan for potential security issues (include hidden + gitignored files,
# where secrets most often live)
ob scan -p secret_patterns.yaml --hidden --no-ignore -o security_audit.txt /project

# Additional pattern collections ship in this repo under config/
ob scan -p config/grammar_patterns.yaml /project  # Grammar checks
```

## Command Reference

### `scan`

Find patterns in files and directories.

Options:

- `-p, --patterns <FILE>` - Path to YAML patterns file (default: patterns.yaml)
- `-o, --output <FILE>` - Output file (default: stdout); it is never scanned itself, even inside a scanned directory
- `-x, --ext <EXTENSIONS>` - Comma-separated file extensions to include
- `-w, --workers <N>` - Number of threads to use (default: logical CPU cores; env: `OOBER_WORKERS`)
- `--hidden` - Include hidden files and directories (skipped by default)
- `--no-ignore` - Do not respect `.gitignore`/`.ignore` files (respected by default)
- `<INPUTS>...` - Files or directories to scan; the same file reached twice is scanned once

Note: By default, hidden files and directories (dotfiles such as `.env`, `.github/`) and anything matched by `.gitignore`/`.ignore` files are NOT scanned. For secret scanning, add `--hidden --no-ignore` so the most likely secret locations are not silently skipped.

Note: Output order is not guaranteed when using multiple workers. Use `-w 1` or sort the output for deterministic ordering.

#### Incremental scanning

`ob scan -i` (`--incremental`) caches per-file scan state so unchanged files are skipped on the next run. `--content-hash` compares content hashes instead of modification timestamps, `--force-full` re-scans everything, and `--clear-cache` deletes the cache first.

The cache is stored at `$XDG_CACHE_HOME/oober/` (default when unset: `~/.cache/oober/` on Linux, `~/Library/Caches/oober/` on macOS), one JSON file per scanned project (the `backups/` directory beside them holds `replace`'s backup records, see [`undo`](#undo)). It contains the matched line contents from previous scans in plaintext - if you scan for secrets with `--incremental`, the matches (including the secrets themselves) are copied into this cache. Treat the cache as sensitive. On Unix the cache directory is kept owner-only (`0700`) and cache files are written with `0600` permissions; use `--clear-cache` to remove a project's cached results.

`--no-cache-content` keeps matched text out of the on-disk cache entirely: only pattern names, file fingerprints, and line numbers are stored, and on a cache hit the matched lines are re-read from the unchanged files to reconstruct the output (if a re-read fails, the file is re-scanned). This keeps the incremental speedup without persisting secrets to disk; it requires `--incremental`.

### `replace`

Replace or remove patterns in files.

The rules come from exactly one of `--preset`, `--config`, or `--pattern` (with `--replacement`); combining them is a usage error.

Options:

- `--preset <PRESET>` - Built-in pattern set: `remove-copyright`, `clean-debug`, `remove-todos`, `trim-whitespace`, `remove-empty-comments`, `tabs-to-spaces`, `spaces-to-tabs`
  - `clean-debug` rewrites a debug call only when it is the whole statement on its own line, replacing it with a comment stub at the same indent (`  console.log(x);` becomes `  // console.log();`). It covers `console.log(...)`, `console.debug(...)`, `console.trace(...)`, `debugger`, Python `print(...)  # DEBUG` and Java `System.out.println(...); // DEBUG`. The call must start the line, its arguments must stay on that line and contain no `;`, and only an optional `;` and whitespace may follow. A call that shares its line with other code (`items.forEach(x => console.log(x));`), a multi-line call and an identifier such as `debuggerEnabled` are left alone. `DEBUG START` ... `DEBUG END` blocks (`//`, `/* */` and `#` forms) are removed. The match is textual, not parsed: a line that starts with a call and continues after its closing parenthesis without a `;` (`console.log(a) || fallback()`) is still treated as one statement, and stubbing the only statement of a brace-less `if` makes the next statement its body, so preview with `--dry-run`.
  - `remove-todos` removes `//` and `#` comments that start with `TODO`, `FIXME`, `HACK` or `XXX`. A trailing comment is cut along with the whitespace before it and the code in front stays; a line holding only the comment is removed whole, line break included. The comment marker must start the line or follow whitespace, so `"http://XXX.example.com"`, `href="#TODO"` and `## TODO` are left alone. The match is textual, not parsed: a marker after whitespace inside a string literal (`"skip // TODO items"`) still counts as a comment, so preview with `--dry-run`.
- `-c, --config <FILE>` - YAML configuration file; the file itself is never processed, even inside an input directory
- `-p, --pattern <PATTERN>` - Single pattern to match (requires `-r`)
- `-r, --replacement <TEXT>` - Replacement text (requires `-p`); to delete the line after each match, use a config file with a `null` replacement
- `-d, --dir <PATH>` - Directory to process (legacy alias for a positional input)
- `-x, --ext <EXTENSIONS>` - File extensions to include
- `-e, --exclude <DIRS>` - Directory names to exclude, matched below each input directory (not against the input's own path); files named as inputs are exempt
- `--no-backup` - Don't create backup files (and skip the existing-backup check below)
- `--dry-run` - Preview changes without applying
- `-w, --workers <N>` - Number of threads to use
- `<INPUTS>...` - Files or directories to process (like `scan`); the same file reached twice is processed once

Before changing a file, `replace` backs it up to `<file>.bak`. If something already sits at that path, the file is changed only when all of this holds: the `.bak` is the regular file oober recorded creating (it still has its recorded size and modification time), and the file still has the size and modification time oober left it with. That is the case for back-to-back runs, so the oldest backup is kept and `undo` still restores the original. In every other case (the `.bak` is your own file, a directory or a symlink, or the file was edited since oober last wrote it) the file is left unchanged, an error names the backup, the other files are still processed, and the run exits non-zero. Changing the file anyway would leave its current state with no backup, and a later `undo` would discard every edit made since the first run. To continue, run `ob undo` (restore the backup) or `ob clean-backups` (remove it; the next run backs up the file as it is now), adding `--all-bak` when oober did not create the `.bak`, or pass `--no-backup`. `--dry-run` makes the same check and reports the files a real run would refuse.

`replace` never processes a symbolic link to a file, whether it is found in a directory or named as an input: each one is reported on stderr (`Skipping symlink: <path>`) and left as a link. The file it points to is edited only when that file is itself among the inputs. A directory given as an input through a symlink is still walked.

Replacement text (`-r`, and `replacements` entries in a config file) can reference the pattern's capture groups: `$1` or `${1}` by index, `${name}` by name. Use the `${1}` form when a letter, digit, or underscore follows the reference: `$1_x` refers to a group named `1_x`, not to group 1 followed by `_x`. Write `$$` for a literal dollar sign. A reference to a group the pattern does not define is an error (in `replace` and `rename` alike), so nothing is changed. Single-quote such arguments so the shell leaves the `$` alone.

### `undo`

Restore files from backups.

Options:

- `-d, --dir <PATH>` - Directory to restore
- `--keep-backups` - Don't delete backup files after restore
- `--all-bak` - Also restore `.bak` files oober has no record of creating
- `--dry-run` - Preview files to be restored (`Would restore <path>`) without changing anything

`undo` restores only the backups that `replace` created. Every time `replace` creates a `<file>.bak`, it writes a small record (the backup's path, size and modification time, the file's path, and the file's size and modification time after the write) to `$XDG_CACHE_HOME/oober/backups/` (the same cache directory as the incremental-scan cache; owner-only on Unix). `undo` and `clean-backups` act on the recorded backups under `--dir`, including the backup of a hidden file that was named as a `replace` input. Any other `*.bak` file in the tree, such as your own `settings.json.bak`, is left alone, and the summary says how many were. A `.bak` at a recorded path counts as oober's only while it has the recorded size and modification time: if you delete oober's backup by hand and later keep a file of your own at that path, that file is left alone too.

Pass `--all-bak` to also act on the unrecorded `*.bak` files found in the tree (hidden files and directories are still skipped). You need it for backups made by oober 0.1.1, after the cache directory was deleted, and for a backup that was modified after oober created it. It restores every such `x.bak` over `x`, so preview with `--all-bak --dry-run` first.

Each file is restored through a temporary file in its own directory and gets the backup's permissions. A restore that fails is reported and the others still run; `undo` then exits non-zero and says how many failed. `undo` refuses to restore over anything that is not a regular file (a directory or a symlink at the file's path) and from a backup that is not a regular file.

### `clean-backups`

Remove backup files without restoring.

Options:

- `-d, --dir <PATH>` - Directory to clean
- `--dry-run` - Preview files to be removed
- `--all-bak` - Also remove `.bak` files oober has no record of creating

Like `undo`, `clean-backups` removes only the backups `replace` recorded creating, and reports how many other `*.bak` files it left alone.

### `rename`

Rename files in a directory.

Options:

- `-d, --dir <PATH>` - Directory to process
- `-p, --pattern <PATTERN>` - Regex pattern to match filenames
- `-r, --replacement <TEXT>` - Replacement string; capture groups as `$1` or `${1}`, `$$` for a literal dollar sign (same rules as `replace`)
- `--dry-run` - Preview changes without applying
- `-w, --workers <N>` - Accepted for compatibility; renames always execute serially so collision checks cannot race

### `completions`

Generate a shell completion script on stdout (see "Shell completions & man page" above for install one-liners).

- `<SHELL>` - One of: `bash`, `zsh`, `fish`, `powershell`, `elvish`

### `manpage`

Print the roff man page on stdout. Takes no arguments.

## Performance

Oober is designed for maximum performance:

- **Parallel processing** - Uses all available CPU cores via Rayon
- **SIMD regex** - Hardware-accelerated pattern matching
- **Efficient I/O** - `scan` streams files line-by-line to minimize memory usage
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

- **Automatic backups** - Creates `.bak` files before any modification; an existing backup is never overwritten, and a file whose `.bak` path is already taken by something else is refused instead of being changed without a backup
- **Atomic writes** - Uses temporary files to prevent corruption
- **Dry-run mode** - Preview all changes before applying
- **Binary detection** - Automatically skips binary files
- **Gitignore respect** - Won't modify ignored files
- **Comprehensive error handling** - Graceful failures with clear messages
- **Local block removal** - Removing a block changes only the blank lines directly next to it, never the rest of the file

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
- `chrono` - Scan report timestamps

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
   - Streaming scan I/O (line-by-line)
   - Minimal memory footprint
   - Smart file filtering
4. **Modularity** - Clean architecture
   - Separate scan/replace/undo logic
   - Reusable pattern configurations
   - Extensible command structure

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Important Notes

- The tool respects `.gitignore` files automatically; `scan` can opt out with `--hidden` / `--no-ignore`
- Binary files are automatically skipped
- Backups are created as `.bak` files before any modifications; an existing `.bak` is never overwritten: `replace` keeps it when it is oober's own backup and the file is unchanged since oober last wrote it, and refuses the file otherwise (see [`replace`](#replace))
- Block removal changes only the blank lines directly next to the removed block; blank lines elsewhere in the file are left as they are
- `scan` matches patterns line-by-line, so scan patterns cannot span lines; `replace` applies patterns to the whole file content — use `(?m)` for per-line `^`/`$` anchors
- Use double backslashes in YAML for regex escapes (e.g., `\\.` for literal dot)

## Recent Enhancements

### Blank Lines Around Removed Blocks

Removing a block (`blocks` in a config, or a preset that removes blocks) changes only the block and the blank lines directly next to it. A blank line holds nothing but spaces and tabs.

- A block that has its lines to itself is removed with its indent and its line break, so no empty line is left where it stood
- When such a block starts the file or follows a blank line, the blank lines directly after it are removed too: a header block leaves no leading blank line, and a block between two paragraphs leaves the blank lines that were above it
- When such a block ends the file, the blank lines directly before it are removed too
- A block that shares its first or last line with other code is cut out of that line; the code and its line break stay
- Everything else is copied byte for byte: blank lines elsewhere in the file (PEP 8 double blank lines, blank lines inside string literals) and every LF or CRLF line ending

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
