# Error Handler Agent

## Mission
Replace lazy `Box<dyn Error>` with proper error types that provide context and help users understand what went wrong.

## Prerequisites
- Module Splitter agent must complete first
- Work from the `refactor/modularize` branch

## Current Problems

1. **Generic errors everywhere**: Line 159: `type Result<T> = std::result::Result<T, Box<dyn Error>>`
2. **Silent failures**: Lines 345-366 - Errors in parallel processing are swallowed
3. **No context**: Users get "No such file" instead of "Config file 'patterns.yaml' not found at /path"
4. **String errors**: Line 281: `return Err("Specify --preset, --config, or --pattern".into())`

## Implementation Plan

### 1. Add thiserror dependency
```toml
# Cargo.toml
[dependencies]
thiserror = "1.0"
anyhow = "1.0"  # For context adding
```

### 2. Create src/errors.rs
```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UberScanError {
    #[error("Failed to read file {path}: {message}")]
    FileRead {
        path: PathBuf,
        message: String,
        #[source]
        source: std::io::Error,
    },
    
    #[error("Failed to write file {path}: {message}")]
    FileWrite {
        path: PathBuf,
        message: String,
        #[source]
        source: std::io::Error,
    },
    
    #[error("Pattern compilation failed for '{pattern}': {message}")]
    PatternCompilation {
        pattern: String,
        message: String,
        #[source]
        source: regex::Error,
    },
    
    #[error("Config file not found: {path}. Searched in: {locations:?}")]
    ConfigNotFound {
        path: PathBuf,
        locations: Vec<PathBuf>,
    },
    
    #[error("Invalid config file {path}: {message}")]
    ConfigParse {
        path: PathBuf,
        message: String,
        #[source]
        source: serde_yaml::Error,
    },
    
    #[error("No input specified. Use --preset, --config, or --pattern")]
    NoInputSpecified,
    
    #[error("File processing failed for {path}: {message}")]
    ProcessingFailed {
        path: PathBuf,
        message: String,
    },
    
    #[error("Backup restoration failed: {message}")]
    BackupFailed {
        message: String,
        #[source]
        source: std::io::Error,
    },
    
    #[error("Multiple errors occurred during parallel processing:\n{messages}")]
    ParallelProcessing {
        messages: String,
    },
}

pub type Result<T> = std::result::Result<T, UberScanError>;

// Helper trait for adding context
pub trait ErrorContext<T> {
    fn context_file(self, path: &Path, operation: &str) -> Result<T>;
    fn context_pattern(self, pattern: &str) -> Result<T>;
}

impl<T> ErrorContext<T> for std::result::Result<T, std::io::Error> {
    fn context_file(self, path: &Path, operation: &str) -> Result<T> {
        self.map_err(|e| match operation {
            "read" => UberScanError::FileRead {
                path: path.to_path_buf(),
                message: format!("Cannot read file"),
                source: e,
            },
            "write" => UberScanError::FileWrite {
                path: path.to_path_buf(),
                message: format!("Cannot write file"),
                source: e,
            },
            _ => UberScanError::ProcessingFailed {
                path: path.to_path_buf(),
                message: format!("{} operation failed", operation),
            },
        })
    }
    
    fn context_pattern(self, pattern: &str) -> Result<T> {
        unreachable!("IO errors don't have pattern context")
    }
}
```

### 3. Fix error handling patterns

#### Before (main.rs:199-201)
```rust
let file = File::open(&patterns_file)?;
let cfg: ScanConfig = serde_yaml::from_reader(file)?;
```

#### After
```rust
let file = File::open(&patterns_file)
    .context_file(&patterns_file, "read")?;
    
let cfg: ScanConfig = serde_yaml::from_reader(file)
    .map_err(|e| UberScanError::ConfigParse {
        path: patterns_file.clone(),
        message: format!("Invalid YAML structure"),
        source: e,
    })?;
```

#### Before (main.rs:206)
```rust
.map(|p| Ok((p.name.clone(), Regex::new(&p.pattern)?)))
```

#### After
```rust
.map(|p| {
    Regex::new(&p.pattern)
        .map(|re| (p.name.clone(), re))
        .map_err(|e| UberScanError::PatternCompilation {
            pattern: p.pattern.clone(),
            message: format!("Invalid regex syntax"),
            source: e,
        })
})
```

### 4. Fix parallel processing error handling

#### Before (main.rs:343-367)
```rust
all_files.par_iter().for_each(|path| {
    if let Ok((changes, modified)) = process_file(...) {
        // Only handles success
    }
});
```

#### After
```rust
let errors: Vec<_> = all_files.par_iter()
    .filter_map(|path| {
        match process_file(...) {
            Ok((changes, modified)) => {
                // Handle success
                None
            }
            Err(e) => Some((path.clone(), e))
        }
    })
    .collect();

if !errors.is_empty() {
    let messages = errors.iter()
        .map(|(path, e)| format!("  {}: {}", path.display(), e))
        .collect::<Vec<_>>()
        .join("\n");
    
    return Err(UberScanError::ParallelProcessing { messages });
}
```

### 5. Improve user-facing messages

#### Config not found
```rust
// Before: generic "file not found"
// After:
Config file 'custom.yaml' not found.
Searched in:
  - /current/dir/custom.yaml
  - /project/root/custom.yaml
  - ~/.uber_scanner/custom.yaml
  - /usr/local/bin/custom.yaml

Try: uber_scanner replace --config ./path/to/config.yaml
```

#### Pattern compilation error
```rust
// Before: "regex parse error"
// After:
Pattern compilation failed for 'console\.log(': 
  Invalid regex syntax: Unclosed group
  
  console\.log(
              ^--- Expected closing parenthesis

Hint: Escape special characters with backslash
```

### 6. Add error recovery strategies

```rust
pub enum RecoveryStrategy {
    Skip,      // Skip failed file, continue processing
    Retry(u8), // Retry N times
    Abort,     // Stop immediately
}

impl UberScanError {
    pub fn recovery_hint(&self) -> &str {
        match self {
            Self::FileRead { .. } => "Check file permissions and path",
            Self::PatternCompilation { .. } => "Verify regex syntax at regex101.com",
            Self::ConfigNotFound { .. } => "Use absolute path or place config in project root",
            _ => "See --help for usage examples",
        }
    }
}
```

## Testing Requirements

Create `tests/error_handling_test.rs`:
```rust
#[test]
fn test_missing_config_provides_search_paths() {
    let result = ConfigLoader::find_config("nonexistent.yaml", "/tmp");
    assert!(result.is_err());
    
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Searched in:"));
}

#[test]
fn test_invalid_regex_shows_pattern() {
    let result = PatternManager::compile("console\\.log(");
    assert!(result.is_err());
    
    let error = result.unwrap_err();
    assert!(error.to_string().contains("console\\.log("));
}

#[test]
fn test_parallel_errors_are_collected() {
    // Test that errors from parallel processing are aggregated
}
```

## Success Criteria

- Zero uses of `Box<dyn Error>`
- All errors have context (file path, pattern, operation)
- Parallel processing errors are collected and reported
- User gets actionable error messages
- Recovery hints provided where applicable
- Tests verify error messages