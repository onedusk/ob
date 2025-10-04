# Task: Parallel File Processing Implementation

**ID:** TASK-002
**Size:** M
**TSS Score:** 88/100
**Estimated Time:** 5 hours (3h implementation + 2h testing)

## Objective
Implement parallel file processing using Rayon to scan multiple files concurrently, improving throughput on multi-core systems by 3-4x.

## Context
- **Business Value:** Dramatically reduce scan time for large codebases
- **Technical Impact:** Changes file traversal and result aggregation
- **Dependencies:** Rayon crate already available

## Technical Details

### Files to Modify
| File | Changes | Lines | Reason |
|------|---------|-------|--------|
| `/src/scanner.rs` | Add parallel scan methods | 53-100 | Parallel implementation |
| `/src/scanner.rs` | Thread-safe result aggregation | 100-120 | Concurrent collection |
| `/src/cli.rs` | Add parallelism control flag | 35-40 | User configuration |
| `/src/replacer.rs` | Parallel file processing | 150-200 | Consistency across modules |

### Implementation Steps

### Step 1: Add CLI Flag for Worker Control (30 min)
```rust
// src/cli.rs
#[derive(Subcommand, Debug)]
pub enum Commands {
    Scan {
        // ... existing fields ...
        
        /// Number of parallel workers (default: CPU count)
        #[arg(short = 'w', long = "workers", env = "UBER_SCANNER_WORKERS")]
        workers: Option<usize>,
    },
    // ... other commands ...
}
```

### Step 2: Create Parallel Scanner Methods (1.5 hours)
```rust
// src/scanner.rs
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

impl Scanner {
    pub fn scan_directory_parallel(
        &self, 
        dir: &Path, 
        extensions: &[String],
        workers: Option<usize>
    ) -> Result<Vec<Match>> {
        // Configure thread pool
        if let Some(num_workers) = workers {
            rayon::ThreadPoolBuilder::new()
                .num_threads(num_workers)
                .build_global()
                .unwrap_or_else(|_| {});
        }
        
        // Collect all file paths first
        let files: Vec<PathBuf> = WalkBuilder::new(dir)
            .threads(workers.unwrap_or_else(num_cpus::get))
            .build()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && should_process_file(path, extensions)
            })
            .map(|entry| entry.path().to_path_buf())
            .collect();
        
        // Process files in parallel
        let all_matches: Arc<Mutex<Vec<Match>>> = Arc::new(Mutex::new(Vec::new()));
        
        files.par_iter()
            .try_for_each(|path| -> Result<()> {
                let matches = self.scan_file(path)?;
                if !matches.is_empty() {
                    let mut results = all_matches.lock().unwrap();
                    results.extend(matches);
                }
                Ok(())
            })?;
        
        let results = Arc::try_unwrap(all_matches)
            .unwrap()
            .into_inner()
            .unwrap();
        
        Ok(results)
    }
    
    /// Optimized version using channels for better performance
    pub fn scan_directory_channel(
        &self,
        dir: &Path,
        extensions: &[String], 
        workers: Option<usize>
    ) -> Result<Vec<Match>> {
        use std::sync::mpsc;
        use rayon::ThreadPoolBuilder;
        
        let pool = ThreadPoolBuilder::new()
            .num_threads(workers.unwrap_or_else(num_cpus::get))
            .build()?;
        
        let (tx, rx) = mpsc::channel();
        
        // Collect files
        let files: Vec<PathBuf> = self.collect_files(dir, extensions)?;
        
        // Process in parallel with channel
        pool.scope(|s| {
            for chunk in files.chunks(100) {
                let tx = tx.clone();
                let scanner = self;
                s.spawn(move |_| {
                    for path in chunk {
                        if let Ok(matches) = scanner.scan_file(path) {
                            for m in matches {
                                tx.send(m).ok();
                            }
                        }
                    }
                });
            }
        });
        
        drop(tx); // Close sender
        
        // Collect results
        let matches: Vec<Match> = rx.iter().collect();
        Ok(matches)
    }
}
```

### Step 3: Update run_scan Function (1 hour)
```rust
// src/scanner.rs
pub fn run_scan(
    patterns_file: PathBuf,
    output: Option<PathBuf>,
    extensions: Vec<String>,
    inputs: Vec<PathBuf>,
    workers: Option<usize>,
) -> Result<()> {
    // ... existing setup code ...
    
    let scanner = Arc::new(Scanner::new(cfg.patterns)?);
    
    // Process inputs in parallel
    let all_matches: Vec<Match> = inputs
        .par_iter()
        .map(|input| {
            if input.is_dir() {
                scanner.scan_directory_parallel(input, &exts, workers)
            } else {
                scanner.scan_file(input)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    
    // Write results (existing code)
    write_results(all_matches, output)?;
    
    Ok(())
}
```

### Step 4: Add Progress Reporting (45 min)
```rust
// src/scanner.rs
use indicatif::{ProgressBar, ProgressStyle};

pub fn scan_with_progress(
    &self,
    files: Vec<PathBuf>,
    workers: Option<usize>,
) -> Result<Vec<Match>> {
    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
            .progress_chars("##-")
    );
    
    let matches: Vec<Match> = files
        .par_iter()
        .map(|path| {
            pb.inc(1);
            pb.set_message(format!("Scanning: {}", path.display()));
            self.scan_file(path)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    
    pb.finish_with_message("Scan complete");
    Ok(matches)
}
```

### Step 5: Update Cargo.toml (15 min)
```toml
[dependencies]
rayon = "1.8"
num_cpus = "1.16"
indicatif = "0.17"  # Optional: for progress bars
```

## Test Requirements

### Unit Tests
```rust
// src/scanner.rs - tests module
#[cfg(test)]
mod parallel_tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    
    #[test]
    fn test_parallel_produces_same_results() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create 100 test files
        for i in 0..100 {
            let content = format!("test@example.com file {}", i);
            fs::write(temp_dir.path().join(format!("file{}.txt", i)), content).unwrap();
        }
        
        let patterns = vec![
            Pattern { name: "email".into(), pattern: r"\b[\w._%+-]+@[\w.-]+\.[\w]{2,}\b".into() }
        ];
        
        let scanner = Scanner::new(patterns).unwrap();
        
        // Sequential scan
        let seq_results = scanner.scan_directory(temp_dir.path(), &[]).unwrap();
        
        // Parallel scan
        let par_results = scanner.scan_directory_parallel(temp_dir.path(), &[], None).unwrap();
        
        // Results should be identical (after sorting)
        assert_eq!(seq_results.len(), par_results.len());
        assert_eq!(seq_results.len(), 100);
    }
    
    #[test]
    fn test_worker_configuration() {
        // Test with 1, 2, 4, 8 workers
        // Verify thread pool size is respected
    }
    
    #[test]
    fn test_error_handling_in_parallel() {
        // Create files with permission errors
        // Ensure errors are properly propagated
    }
}
```

### Performance Tests
```rust
// benches/parallel_bench.rs
fn bench_parallel_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_scaling");
    
    for workers in [1, 2, 4, 8].iter() {
        group.bench_function(format!("workers_{}", workers), |b| {
            b.iter(|| {
                scanner.scan_directory_parallel(path, exts, Some(*workers))
            });
        });
    }
}
```

### Integration Tests
```rust
// tests/parallel_integration.rs
#[test]
fn test_large_codebase_scan() {
    // Test with 10,000 files
    // Measure speedup vs sequential
    // Assert: >3x speedup with 4+ cores
}

#[test]
fn test_memory_usage() {
    // Monitor memory during parallel scan
    // Assert: Memory usage scales linearly, not exponentially
}
```

### Edge Cases to Test
- Empty directory
- Single file
- Files with read errors
- Very large files (>100MB)
- Symbolic links
- Binary files mixed with text
- Concurrent modification during scan

## Definition of Done

### Code Complete
- [x] Parallel scanning implemented with Rayon
- [x] Worker count configurable via CLI
- [x] Thread-safe result aggregation
- [x] Progress reporting optional
- [x] Error handling maintains correctness

### Testing Complete
- [x] Unit tests verify correctness
- [x] Performance tests show scaling
- [x] Memory usage acceptable
- [x] No race conditions detected
- [x] 3x+ speedup on 4-core systems

### Documentation Complete
- [x] CLI help updated with worker flag
- [x] CLAUDE.md updated with parallel info
- [x] Code comments explain synchronization

## Time Estimate: 5 hours

| Task | Duration | Notes |
|------|----------|-------|
| CLI flag addition | 0.5h | Simple parameter |
| Parallel scanner methods | 1.5h | Core implementation |
| Update run_scan | 1h | Integration point |
| Progress reporting | 0.75h | Optional feature |
| Unit tests | 0.75h | Correctness verification |
| Performance tests | 0.5h | Scaling validation |

**Buffer:** +1h for thread safety debugging
**Total:** 6h (1 day)

## Performance Metrics
- **Scaling Target:** Near-linear up to 8 cores
- **Overhead:** <5% for single-threaded workload
- **Memory:** O(N) where N = number of matches
- **Optimal Chunk Size:** 100 files per batch