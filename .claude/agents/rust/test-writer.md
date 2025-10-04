# Test Writer Agent

## Mission
Add comprehensive test coverage to a codebase that currently has ZERO tests. Target 80%+ coverage with focus on edge cases.

## Prerequisites
- Module Splitter agent must complete first
- Work from the `refactor/modularize` branch

## Test Structure

### Unit Tests (in each module)

#### src/scanner.rs
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_pattern_matching() {
        let scanner = Scanner::new(vec![
            Pattern { name: "todo".into(), pattern: "TODO".into() }
        ]).unwrap();
        
        let content = "// TODO: Fix this\nNormal line\n// TODO: Another";
        let matches = scanner.scan_content(content);
        assert_eq!(matches.len(), 2);
    }
    
    #[test]
    fn test_regex_patterns() {
        let scanner = Scanner::new(vec![
            Pattern { name: "console".into(), pattern: r"console\.\w+\([^)]*\)".into() }
        ]).unwrap();
        
        let content = "console.log('test');\nconsole.error('fail');";
        let matches = scanner.scan_content(content);
        assert_eq!(matches.len(), 2);
    }
    
    #[test]
    fn test_empty_file() {
        let scanner = Scanner::new(vec![
            Pattern { name: "any".into(), pattern: ".*".into() }
        ]).unwrap();
        
        let matches = scanner.scan_content("");
        assert_eq!(matches.len(), 0);
    }
    
    #[test]
    fn test_binary_file_skipped() {
        // Create binary file with null bytes
        let content = b"\x00\xFF\xDE\xAD\xBE\xEF";
        // Scanner should detect and skip binary
    }
}
```

#### src/replacer.rs
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_replacement() {
        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec!["TODO".into()],
            replacements: vec![Some("DONE".into())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        }).unwrap();
        
        let content = "// TODO: Fix this";
        let result = replacer.process_content(content);
        assert_eq!(result, "// DONE: Fix this");
    }
    
    #[test]
    fn test_delete_next_line() {
        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec!["DELETE_NEXT".into()],
            replacements: vec![None], // None means delete next line
            blocks: vec![],
            extensions: None,
            exclude: None,
        }).unwrap();
        
        let content = "Line 1\nDELETE_NEXT\nLine 2\nLine 3";
        let result = replacer.process_content(content);
        assert_eq!(result, "Line 1\nDELETE_NEXT\nLine 3");
    }
    
    #[test]
    fn test_block_removal() {
        let replacer = Replacer::new(ReplaceConfig {
            patterns: vec![],
            replacements: vec![],
            blocks: vec![Block {
                start: "/* START */".into(),
                end: "/* END */".into(),
            }],
            extensions: None,
            exclude: None,
        }).unwrap();
        
        let content = "Before\n/* START */\nMiddle\n/* END */\nAfter";
        let result = replacer.process_content(content);
        assert_eq!(result, "Before\nAfter");
    }
    
    #[test]
    fn test_empty_line_cleanup() {
        let content = "\n\n\nStart\n\n\n\nMiddle\n\n\nEnd\n";
        let result = clean_empty_lines(content);
        assert_eq!(result, "Start\n\nMiddle\n\nEnd\n");
    }
    
    #[test]
    fn test_backup_creation() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "original").unwrap();
        
        let replacer = Replacer::new(/* config */).unwrap();
        replacer.process_file(&file, ProcessOptions {
            create_backup: true,
            dry_run: false,
        }).unwrap();
        
        let backup = dir.path().join("test.txt.bak");
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "original");
    }
}
```

#### src/patterns.rs
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pattern_compilation() {
        let mut manager = PatternManager::new();
        
        let regex1 = manager.compile("TODO").unwrap();
        let regex2 = manager.compile("TODO").unwrap();
        
        // Should return cached version
        assert!(std::ptr::eq(regex1, regex2));
    }
    
    #[test]
    fn test_invalid_pattern() {
        let mut manager = PatternManager::new();
        let result = manager.compile("console\\.log(");
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unclosed group"));
    }
    
    #[test]
    fn test_preset_loading() {
        let manager = PatternManager::new();
        
        let config = manager.load_preset(Preset::CleanDebug).unwrap();
        assert!(!config.patterns.is_empty());
        assert!(config.patterns.iter().any(|p| p.contains("console")));
    }
}
```

### Integration Tests

#### tests/integration_test.rs
```rust
use tempfile::TempDir;
use std::fs;
use uber_scanner::{Scanner, Replacer, Config};

#[test]
fn test_end_to_end_scan() {
    let dir = TempDir::new().unwrap();
    
    // Create test files
    fs::write(dir.path().join("file1.js"), "console.log('test');\n// TODO: fix").unwrap();
    fs::write(dir.path().join("file2.py"), "print('debug')\n# TODO: remove").unwrap();
    fs::write(dir.path().join("ignored.txt"), "TODO: should be ignored").unwrap();
    
    // Create patterns config
    let patterns = r#"
patterns:
  - name: todo
    pattern: 'TODO'
  - name: console
    pattern: 'console\.log'
"#;
    fs::write(dir.path().join("patterns.yaml"), patterns).unwrap();
    
    // Run scan
    let output = Command::new("cargo")
        .args(&["run", "--", "scan", "-p", "patterns.yaml", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[todo] file1.js:2"));
    assert!(stdout.contains("[console] file1.js:1"));
    assert!(stdout.contains("[todo] file2.py:2"));
}

#[test]
fn test_end_to_end_replace_with_backup() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.js");
    
    fs::write(&file, "console.log('debug');\nproduction_code();").unwrap();
    
    // Run replacement
    let output = Command::new("cargo")
        .args(&["run", "--", "replace", 
                "--dir", ".",
                "--pattern", "console\\.log\\(.*\\)",
                "--replacement", "// console.log()"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    
    // Check file was modified
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "// console.log();\nproduction_code();");
    
    // Check backup exists
    assert!(dir.path().join("test.js.bak").exists());
    
    // Test undo
    let output = Command::new("cargo")
        .args(&["run", "--", "undo", "--dir", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    
    let restored = fs::read_to_string(&file).unwrap();
    assert_eq!(restored, "console.log('debug');\nproduction_code();");
}

#[test]
fn test_gitignore_respected() {
    let dir = TempDir::new().unwrap();
    
    // Create .gitignore
    fs::write(dir.path().join(".gitignore"), "node_modules/\n*.log").unwrap();
    
    // Create files
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules/test.js"), "TODO").unwrap();
    fs::write(dir.path().join("error.log"), "TODO").unwrap();
    fs::write(dir.path().join("app.js"), "TODO").unwrap();
    
    // Run scan
    let scanner = Scanner::new(/* patterns */).unwrap();
    let results = scanner.scan_directory(dir.path(), &[]).unwrap();
    
    // Should only find TODO in app.js
    assert_eq!(results.len(), 1);
    assert!(results[0].path.ends_with("app.js"));
}
```

### Edge Case Tests

#### tests/edge_cases_test.rs
```rust
#[test]
fn test_symlink_handling() {
    // Symlinks should be skipped
}

#[test]
fn test_permission_denied() {
    // Files without read permission should error gracefully
}

#[test]
fn test_unicode_content() {
    let content = "// TODO: 修复这个 🔧\nconst emoji = '😀';";
    // Should handle UTF-8 properly
}

#[test]
fn test_large_file() {
    // Create 100MB file
    // Should stream process without loading entire file
}

#[test]
fn test_concurrent_modification() {
    // File modified during processing
}

#[test]
fn test_disk_full() {
    // Backup creation when disk is full
}

#[test]
fn test_invalid_regex_in_config() {
    // Config with broken regex patterns
}

#[test]
fn test_circular_symlinks() {
    // Directory with circular symlink references
}
```

### Performance Tests

#### tests/performance_test.rs
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_scan_large_codebase(c: &mut Criterion) {
    // Setup large test directory
    
    c.bench_function("scan 1000 files", |b| {
        b.iter(|| {
            Scanner::scan_directory(black_box(path))
        });
    });
}

fn bench_parallel_vs_serial(c: &mut Criterion) {
    c.bench_function("parallel processing", |b| {
        // Benchmark with rayon
    });
    
    c.bench_function("serial processing", |b| {
        // Benchmark without rayon
    });
}

fn bench_regex_compilation(c: &mut Criterion) {
    c.bench_function("compile 100 patterns", |b| {
        // Benchmark pattern compilation
    });
}
```

### Test Fixtures

#### tests/fixtures/
```
tests/fixtures/
├── patterns/
│   ├── valid_patterns.yaml
│   ├── invalid_patterns.yaml
│   └── edge_case_patterns.yaml
├── sample_code/
│   ├── javascript.js
│   ├── python.py
│   ├── rust.rs
│   └── mixed_encodings.txt
└── configs/
    ├── minimal.yaml
    ├── complex.yaml
    └── broken.yaml
```

## Test Coverage Requirements

Run coverage with:
```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out Html --output-dir coverage
```

Target coverage:
- scanner.rs: 85%+
- replacer.rs: 85%+
- patterns.rs: 90%+
- config.rs: 80%+
- errors.rs: 70%+
- Overall: 80%+

## Success Criteria

- All tests pass: `cargo test`
- Coverage meets targets: `cargo tarpaulin`
- Tests run in < 10 seconds
- No flaky tests
- Edge cases covered
- Performance benchmarks established