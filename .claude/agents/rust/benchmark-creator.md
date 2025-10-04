# Benchmark Creator Agent

## Mission
Add performance benchmarks to validate "ultra-fast" claims and establish baseline metrics.

## Prerequisites
- Module Splitter and Pattern Optimizer must complete first
- Work from merged refactor branches

## Current State
- Zero benchmarks exist
- Performance claims unsubstantiated
- No comparison with alternatives (ripgrep, sd)

## Implementation Plan

### 1. Add Benchmark Dependencies

```toml
# Cargo.toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3.8"
rand = "0.8"

[[bench]]
name = "scanner_bench"
harness = false

[[bench]]
name = "replacer_bench"
harness = false

[[bench]]
name = "comparison_bench"
harness = false
```

### 2. Scanner Benchmarks (benches/scanner_bench.rs)

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use uber_scanner::{Scanner, Pattern};
use tempfile::TempDir;
use std::fs;
use std::io::Write;

fn generate_test_file(lines: usize, pattern_density: f32) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut content = String::new();
    
    for i in 0..lines {
        if rng.gen::<f32>() < pattern_density {
            content.push_str(&format!("console.log('Line {}'); // TODO: fix this\n", i));
        } else {
            content.push_str(&format!("normal_code_line({});\n", i));
        }
    }
    
    content
}

fn bench_scan_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("scanner");
    
    // Test different file sizes
    for size in [100, 1000, 10000, 100000].iter() {
        let content = generate_test_file(*size, 0.1); // 10% pattern density
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.js");
        fs::write(&file_path, &content).unwrap();
        
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_lines", size)),
            size,
            |b, _| {
                let scanner = Scanner::new(vec![
                    Pattern { name: "console".into(), pattern: r"console\.\w+".into() },
                    Pattern { name: "todo".into(), pattern: "TODO".into() },
                ]).unwrap();
                
                b.iter(|| {
                    scanner.scan_file(black_box(&file_path))
                });
            },
        );
    }
    
    group.finish();
}

fn bench_pattern_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_complexity");
    
    let content = generate_test_file(10000, 0.2);
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.js");
    fs::write(&file_path, &content).unwrap();
    
    // Simple literal pattern
    group.bench_function("literal", |b| {
        let scanner = Scanner::new(vec![
            Pattern { name: "todo".into(), pattern: "TODO".into() }
        ]).unwrap();
        
        b.iter(|| scanner.scan_file(black_box(&file_path)));
    });
    
    // Simple regex
    group.bench_function("simple_regex", |b| {
        let scanner = Scanner::new(vec![
            Pattern { name: "console".into(), pattern: r"console\.\w+".into() }
        ]).unwrap();
        
        b.iter(|| scanner.scan_file(black_box(&file_path)));
    });
    
    // Complex regex with lookahead
    group.bench_function("complex_regex", |b| {
        let scanner = Scanner::new(vec![
            Pattern { name: "complex".into(), 
                     pattern: r"(?=.*console)(?=.*log).*\(\s*['\"].*['\"].*\)".into() }
        ]).unwrap();
        
        b.iter(|| scanner.scan_file(black_box(&file_path)));
    });
    
    // Multiple patterns
    group.bench_function("10_patterns", |b| {
        let patterns = (0..10).map(|i| {
            Pattern { 
                name: format!("pattern_{}", i),
                pattern: format!(r"pattern_{}\w*", i),
            }
        }).collect();
        
        let scanner = Scanner::new(patterns).unwrap();
        b.iter(|| scanner.scan_file(black_box(&file_path)));
    });
    
    group.finish();
}

fn bench_parallel_vs_serial(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallelism");
    
    // Create 100 test files
    let temp_dir = TempDir::new().unwrap();
    for i in 0..100 {
        let content = generate_test_file(1000, 0.15);
        fs::write(temp_dir.path().join(format!("file_{}.js", i)), content).unwrap();
    }
    
    let scanner = Scanner::new(vec![
        Pattern { name: "console".into(), pattern: r"console\.\w+".into() },
        Pattern { name: "todo".into(), pattern: "TODO".into() },
    ]).unwrap();
    
    group.bench_function("serial", |b| {
        b.iter(|| {
            scanner.scan_directory_serial(black_box(temp_dir.path()))
        });
    });
    
    group.bench_function("parallel_2_threads", |b| {
        b.iter(|| {
            scanner.scan_directory_parallel(black_box(temp_dir.path()), 2)
        });
    });
    
    group.bench_function("parallel_4_threads", |b| {
        b.iter(|| {
            scanner.scan_directory_parallel(black_box(temp_dir.path()), 4)
        });
    });
    
    group.bench_function("parallel_8_threads", |b| {
        b.iter(|| {
            scanner.scan_directory_parallel(black_box(temp_dir.path()), 8)
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_scan_performance,
    bench_pattern_complexity,
    bench_parallel_vs_serial
);
criterion_main!(benches);
```

### 3. Replacer Benchmarks (benches/replacer_bench.rs)

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use uber_scanner::{Replacer, ReplaceConfig};

fn bench_replacement_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("replacer");
    
    let content = generate_test_content(10000);
    
    // Simple string replacement
    group.bench_function("simple_replace", |b| {
        let config = ReplaceConfig {
            patterns: vec!["TODO".into()],
            replacements: vec![Some("DONE".into())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        };
        
        let replacer = Replacer::new(config).unwrap();
        
        b.iter(|| {
            replacer.process_content(black_box(&content))
        });
    });
    
    // Regex replacement
    group.bench_function("regex_replace", |b| {
        let config = ReplaceConfig {
            patterns: vec![r"console\.\w+\([^)]*\)".into()],
            replacements: vec![Some("// removed".into())],
            blocks: vec![],
            extensions: None,
            exclude: None,
        };
        
        let replacer = Replacer::new(config).unwrap();
        
        b.iter(|| {
            replacer.process_content(black_box(&content))
        });
    });
    
    // Block removal
    group.bench_function("block_removal", |b| {
        let config = ReplaceConfig {
            patterns: vec![],
            replacements: vec![],
            blocks: vec![
                Block {
                    start: "/* START */".into(),
                    end: "/* END */".into(),
                }
            ],
            extensions: None,
            exclude: None,
        };
        
        let replacer = Replacer::new(config).unwrap();
        
        b.iter(|| {
            replacer.process_content(black_box(&content))
        });
    });
    
    // Multiple operations
    group.bench_function("combined_operations", |b| {
        let config = ReplaceConfig {
            patterns: vec![
                "TODO".into(),
                r"console\.\w+".into(),
                "FIXME".into(),
            ],
            replacements: vec![
                Some("DONE".into()),
                Some("// console".into()),
                Some("".into()),
            ],
            blocks: vec![
                Block {
                    start: "/* DEBUG START */".into(),
                    end: "/* DEBUG END */".into(),
                }
            ],
            extensions: None,
            exclude: None,
        };
        
        let replacer = Replacer::new(config).unwrap();
        
        b.iter(|| {
            replacer.process_content(black_box(&content))
        });
    });
    
    group.finish();
}

fn bench_file_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_operations");
    
    let temp_dir = TempDir::new().unwrap();
    
    // Benchmark backup creation
    group.bench_function("create_backup", |b| {
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "test content").unwrap();
        
        b.iter(|| {
            create_backup(black_box(&file_path));
            // Clean up
            fs::remove_file(format!("{}.bak", file_path.display())).ok();
        });
    });
    
    // Benchmark atomic write
    group.bench_function("atomic_write", |b| {
        let file_path = temp_dir.path().join("test.txt");
        let content = "new content".to_string();
        
        b.iter(|| {
            atomic_write(black_box(&file_path), black_box(&content))
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_replacement_strategies,
    bench_file_operations
);
criterion_main!(benches);
```

### 4. Comparison Benchmarks (benches/comparison_bench.rs)

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::process::Command;

fn bench_vs_ripgrep(c: &mut Criterion) {
    let mut group = c.benchmark_group("vs_ripgrep");
    
    let temp_dir = create_test_directory(1000); // 1000 files
    
    group.bench_function("uber_scanner", |b| {
        b.iter(|| {
            Command::new("./target/release/uber_scanner")
                .args(&["scan", "-p", "patterns.yaml"])
                .arg(black_box(temp_dir.path()))
                .output()
                .unwrap();
        });
    });
    
    group.bench_function("ripgrep", |b| {
        b.iter(|| {
            Command::new("rg")
                .args(&["TODO|console\\.log"])
                .arg(black_box(temp_dir.path()))
                .output()
                .unwrap();
        });
    });
    
    group.finish();
}

fn bench_vs_sd(c: &mut Criterion) {
    let mut group = c.benchmark_group("vs_sd");
    
    let temp_dir = create_test_directory(100);
    
    group.bench_function("uber_scanner_replace", |b| {
        b.iter(|| {
            // Copy files for fair comparison
            let work_dir = copy_directory(&temp_dir);
            
            Command::new("./target/release/uber_scanner")
                .args(&["replace", "--pattern", "TODO", "--replacement", "DONE"])
                .arg("--dir")
                .arg(work_dir.path())
                .output()
                .unwrap();
        });
    });
    
    group.bench_function("sd", |b| {
        b.iter(|| {
            let work_dir = copy_directory(&temp_dir);
            
            Command::new("sd")
                .args(&["TODO", "DONE"])
                .arg("**/*.js")
                .current_dir(work_dir.path())
                .output()
                .unwrap();
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_vs_ripgrep,
    bench_vs_sd
);
criterion_main!(benches);
```

### 5. Memory Benchmarks (benches/memory_bench.rs)

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct MemoryTracker;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for MemoryTracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = System.alloc(layout);
        if !ret.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::SeqCst);
        }
        ret
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        ALLOCATED.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static GLOBAL: MemoryTracker = MemoryTracker;

fn measure_memory_usage() {
    let before = ALLOCATED.load(Ordering::SeqCst);
    
    // Run scanner on large directory
    let scanner = Scanner::new(/* patterns */).unwrap();
    scanner.scan_directory("/large/codebase").unwrap();
    
    let after = ALLOCATED.load(Ordering::SeqCst);
    let used = after - before;
    
    println!("Memory used: {} MB", used as f64 / 1_048_576.0);
}
```

### 6. Performance Report Generator

```rust
// scripts/generate_performance_report.rs
use std::fs;
use std::process::Command;

fn main() {
    println!("Running benchmarks...\n");
    
    // Run all benchmarks
    Command::new("cargo")
        .args(&["bench", "--", "--save-baseline", "current"])
        .status()
        .unwrap();
    
    // Generate report
    let mut report = String::new();
    report.push_str("# Performance Report\n\n");
    report.push_str(&format!("Date: {}\n", chrono::Local::now()));
    report.push_str(&format!("Commit: {}\n\n", get_git_hash()));
    
    // Parse criterion output
    let criterion_dir = "target/criterion";
    for entry in fs::read_dir(criterion_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            let name = entry.file_name();
            report.push_str(&format!("## {}\n", name.to_string_lossy()));
            
            // Read benchmark results
            let estimates = fs::read_to_string(
                entry.path().join("current/estimates.json")
            ).unwrap();
            
            // Parse and format results
            // ...
        }
    }
    
    // Compare with alternatives
    report.push_str("\n## Comparison with Alternatives\n\n");
    report.push_str("| Tool | Task | Time (ms) | Memory (MB) |\n");
    report.push_str("|------|------|-----------|-------------|\n");
    report.push_str("| uber_scanner | Scan 1000 files | X | Y |\n");
    report.push_str("| ripgrep | Scan 1000 files | X | Y |\n");
    report.push_str("| uber_scanner | Replace in 100 files | X | Y |\n");
    report.push_str("| sd | Replace in 100 files | X | Y |\n");
    
    fs::write("PERFORMANCE.md", report).unwrap();
    println!("Report generated: PERFORMANCE.md");
}
```

### 7. CI Integration

```yaml
# .github/workflows/benchmark.yml
name: Benchmarks

on:
  pull_request:
  push:
    branches: [main]

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          
      - name: Install tools
        run: |
          cargo install ripgrep
          cargo install sd
          
      - name: Run benchmarks
        run: cargo bench --all
        
      - name: Generate report
        run: cargo run --bin generate_performance_report
        
      - name: Upload results
        uses: actions/upload-artifact@v3
        with:
          name: benchmark-results
          path: |
            target/criterion/**/*.html
            PERFORMANCE.md
            
      - name: Comment on PR
        if: github.event_name == 'pull_request'
        uses: actions/github-script@v6
        with:
          script: |
            const fs = require('fs');
            const report = fs.readFileSync('PERFORMANCE.md', 'utf8');
            github.rest.issues.createComment({
              issue_number: context.issue.number,
              owner: context.repo.owner,
              repo: context.repo.repo,
              body: report
            });
```

## Success Criteria

- Benchmarks run with `cargo bench`
- HTML reports generated in target/criterion/
- Performance regression detection
- Memory usage under 50MB for typical workloads
- Comparison shows competitive performance with ripgrep/sd
- CI runs benchmarks on every PR
- PERFORMANCE.md auto-generated with results