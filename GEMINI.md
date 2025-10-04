# Oober: High-Performance Pattern Scanning and Replacement Tool

## Project Overview

Oober is a command-line tool built in Rust for high-performance pattern scanning and replacement in large codebases. It leverages SIMD-powered regex, parallel file processing, and atomic writes to provide a fast, safe, and efficient way to manage code at scale.

The tool is designed with reliability as a top priority, featuring automatic backups, undo functionality, and smart cleanup of empty lines. It is configured through YAML files, allowing for reusable and complex pattern definitions.

**Key Features:**

* **Ultra-fast scanning:** Uses a SIMD-powered regex engine and parallel file processing with Rayon.
* **Safe replacements:** Creates `.bak` files before any modifications and uses atomic writes to prevent data corruption.
* **Undo support:** Allows for easy restoration of files from backups.
* **Configuration-driven:** Patterns and replacement rules are defined in YAML files.
* **Gitignore aware:** Automatically respects `.gitignore` files.

**Core Technologies:**

* **Language:** Rust (2024 Edition)
* **CLI Parsing:** `clap`
* **Pattern Matching:** `regex`
* **Parallelism:** `rayon`
* **Serialization:** `serde` (for YAML and JSON)
* **Error Handling:** `thiserror`, `anyhow`

## Building and Running

### Build

To build the optimized release binary, run:

```bash
cargo build --release
```

The executable will be located at `target/release/oober`.

### Running

The tool has four main commands: `scan`, `replace`, `undo`, and `clean-backups`.

**Scan for patterns:**

```bash
# Scan a directory with a specific patterns file
./target/release/oober scan --patterns patterns/secret_patterns.yaml /path/to/scan
```

**Replace patterns:**

```bash
# Perform a dry-run replacement to preview changes
./target/release/oober replace --config replace_config.yaml --dir /path/to/project --dry-run

# Apply the replacement
./target/release/oober replace --config replace_config.yaml --dir /path/to/project
```

**Undo a replacement:**

```bash
./target/release/oober undo --dir /path/to/project
```

**Clean up backups:**

```bash
./target/release/oober clean-backups --dir /path/to/project
```

### Testing

To run the test suite:

```bash
cargo test
```

## Development Conventions

* **Modularity:** The application is structured into modules for different functionalities (`cli`, `scanner`, `replacer`, `errors`, etc.).
* **Error Handling:** Uses `thiserror` for custom error types and `anyhow` for application-level error handling.
* **Configuration:** All pattern and replacement logic is managed through external YAML files to keep the core logic decoupled from the data.
* **Safety:** Prioritizes data integrity through atomic writes and automatic backups.
* **Performance:** Leverages parallelism and optimized libraries to ensure high-speed execution.
