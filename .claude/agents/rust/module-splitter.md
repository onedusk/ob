# Module Splitter Agent

## Mission
Break the 847-line main.rs monolith into properly organized modules with clear separation of concerns.

## Current State Analysis
- main.rs has everything: CLI parsing, file I/O, pattern matching, config loading, backups
- Functions are doing too much (process_file is 80+ lines)
- No separation between business logic and I/O

## Required Module Structure

### src/lib.rs
```rust
pub mod cli;
pub mod scanner;
pub mod replacer;
pub mod patterns;
pub mod config;
pub mod errors;

// Re-export main types
pub use errors::{Result, Error};
pub use scanner::Scanner;
pub use replacer::Replacer;
```

### src/cli.rs
Move from main.rs:
- Lines 17-128: All clap structures (Args, Commands, Preset)
- Keep argument parsing logic
- Export: `pub fn parse_args() -> Args`

### src/scanner.rs  
Move from main.rs:
- Lines 186-246: scan_command and scan_file functions
- Create Scanner struct to hold compiled patterns
- Implement:
  ```rust
  pub struct Scanner {
      patterns: Vec<(String, Regex)>,
  }
  
  impl Scanner {
      pub fn new(patterns: Vec<Pattern>) -> Result<Self>
      pub fn scan_file(&self, path: &Path) -> Result<Vec<Match>>
      pub fn scan_directory(&self, dir: &Path, extensions: &[String]) -> Result<Vec<Match>>
  }
  ```

### src/replacer.rs
Move from main.rs:
- Lines 248-376: replace_command
- Lines 378-458: process_file  
- Lines 460-483: undo_command
- Lines 801-846: clean_backups_command
- Lines 496-551: clean_empty_lines

Create proper structures:
```rust
pub struct Replacer {
    patterns: Vec<Regex>,
    replacements: Vec<Option<String>>,
    blocks: Vec<BlockPattern>,
}

impl Replacer {
    pub fn new(config: ReplaceConfig) -> Result<Self>
    pub fn process_file(&self, path: &Path, options: ProcessOptions) -> Result<ProcessResult>
    pub fn undo(&self, dir: &Path, keep_backups: bool) -> Result<UndoStats>
}
```

### src/patterns.rs
Move from main.rs:
- Lines 130-157: Pattern structures
- Lines 553-723: get_preset_config function (refactor this mess)
- Add pattern compilation and caching

```rust
pub struct PatternManager {
    cache: HashMap<String, Regex>,
}

impl PatternManager {
    pub fn compile(&mut self, pattern: &str) -> Result<&Regex>
    pub fn load_preset(&self, preset: Preset) -> Result<ReplaceConfig>
}
```

### src/config.rs
Move from main.rs:
- Lines 725-799: find_config_file function
- Add config validation

```rust
pub struct ConfigLoader;

impl ConfigLoader {
    pub fn find_config(path: &Path, working_dir: &Path) -> Result<PathBuf>
    pub fn load_scan_config(path: &Path) -> Result<ScanConfig>
    pub fn load_replace_config(path: &Path) -> Result<ReplaceConfig>
}
```

### src/errors.rs
New file with proper error types:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Pattern compilation failed: {0}")]
    Regex(#[from] regex::Error),
    
    #[error("Config error: {0}")]
    Config(String),
    
    #[error("File processing failed: {path}")]
    Processing { path: PathBuf, source: Box<dyn std::error::Error> },
}

pub type Result<T> = std::result::Result<T, Error>;
```

### src/main.rs (new, minimal)
```rust
mod cli;
mod scanner;
mod replacer;
mod patterns;
mod config;
mod errors;

use errors::Result;

fn main() -> Result<()> {
    let args = cli::parse_args();
    
    match args.command {
        cli::Commands::Scan { .. } => scanner::run_scan(args),
        cli::Commands::Replace { .. } => replacer::run_replace(args),
        cli::Commands::Undo { .. } => replacer::run_undo(args),
        cli::Commands::CleanBackups { .. } => replacer::run_clean_backups(args),
    }
}
```

## Refactoring Steps

1. Create all module files with basic structure
2. Move type definitions first (structs, enums)
3. Move functions into appropriate modules
4. Fix imports and visibility
5. Create proper public APIs for each module
6. Ensure all tests still pass (write basic tests if none exist)

## Success Criteria

- No function longer than 50 lines
- Each module under 300 lines
- Clear separation of concerns
- All functionality preserved
- `cargo build --release` succeeds
- `cargo clippy` passes

## Anti-patterns to Fix

- Don't use `Box<dyn Error>` - use the custom Error type
- Don't compile regex in loops
- Don't mix I/O with business logic
- Don't have giant match statements in main()