# Task: Incremental Scanning Implementation

**ID:** TASK-004
**Size:** M
**TSS Score:** 90/100
**Estimated Time:** 6 hours (4h implementation + 2h testing)

## Objective
Implement incremental scanning that tracks file modifications and only rescans changed files, achieving 80-90% performance improvement for repeated scans.

## Context
- **Business Value:** Dramatic speedup for CI/CD pipelines and watch mode
- **Technical Impact:** New state persistence layer, file fingerprinting system
- **Dependencies:** Requires filesystem metadata access, cache storage

## Technical Details

### Files to Modify
| File | Changes | Lines | Reason |
|------|---------|-------|--------|
| `/src/state_manager.rs` | New module | 0-250 | State persistence |
| `/src/lib.rs` | Add module export | 9-10 | Module registration |
| `/src/scanner.rs` | Add incremental methods | 120-200 | Incremental logic |
| `/src/cli.rs` | Add incremental flag | 40-45 | CLI parameter |
| `/src/fingerprint.rs` | New module | 0-150 | File fingerprinting |

### New Components

#### State Manager Module
```rust
// src/state_manager.rs
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs;
use std::time::SystemTime;
use serde::{Serialize, Deserialize};
use crate::errors::Result;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanState {
    pub version: String,
    pub last_scan: SystemTime,
    pub files: HashMap<PathBuf, FileState>,
    pub patterns_hash: String,
    pub scan_results: HashMap<PathBuf, Vec<CachedMatch>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
    pub hash: String,
    pub last_scanned: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMatch {
    pub pattern_name: String,
    pub line_number: usize,
    pub line_content: String,
}

pub struct StateManager {
    state_dir: PathBuf,
    project_id: String,
}

impl StateManager {
    pub fn new(project_root: &Path) -> Result<Self> {
        let state_dir = Self::get_state_dir()?;
        let project_id = Self::compute_project_id(project_root)?;
        
        // Ensure state directory exists
        fs::create_dir_all(&state_dir)?;
        
        Ok(Self {
            state_dir,
            project_id,
        })
    }
    
    fn get_state_dir() -> Result<PathBuf> {
        // Try XDG_CACHE_HOME first, then fallback
        let cache_dir = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::cache_dir()
                    .unwrap_or_else(|| PathBuf::from(".cache"))
            });
        
        Ok(cache_dir.join("uber_scanner"))
    }
    
    fn compute_project_id(project_root: &Path) -> Result<String> {
        use sha2::{Sha256, Digest};
        
        let canonical = project_root.canonicalize()?;
        let path_str = canonical.to_string_lossy();
        
        let mut hasher = Sha256::new();
        hasher.update(path_str.as_bytes());
        let result = hasher.finalize();
        
        Ok(format!("{:x}", result))
    }
    
    pub fn load_state(&self) -> Result<Option<ScanState>> {
        let state_file = self.state_file_path();
        
        if !state_file.exists() {
            return Ok(None);
        }
        
        let contents = fs::read_to_string(&state_file)?;
        let state: ScanState = serde_json::from_str(&contents)?;
        
        // Validate version compatibility
        if state.version != env!("CARGO_PKG_VERSION") {
            // Version mismatch, invalidate cache
            return Ok(None);
        }
        
        Ok(Some(state))
    }
    
    pub fn save_state(&self, state: &ScanState) -> Result<()> {
        let state_file = self.state_file_path();
        let contents = serde_json::to_string_pretty(state)?;
        
        // Atomic write using tempfile
        use tempfile::NamedTempFile;
        let mut temp_file = NamedTempFile::new_in(&self.state_dir)?;
        temp_file.write_all(contents.as_bytes())?;
        temp_file.persist(state_file)?;
        
        Ok(())
    }
    
    fn state_file_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}.json", self.project_id))
    }
    
    pub fn clear_cache(&self) -> Result<()> {
        let state_file = self.state_file_path();
        if state_file.exists() {
            fs::remove_file(state_file)?;
        }
        Ok(())
    }
}
```

#### File Fingerprinting Module
```rust
// src/fingerprint.rs
use std::path::Path;
use std::fs::{File, Metadata};
use std::io::{BufReader, Read};
use std::time::SystemTime;
use sha2::{Sha256, Digest};
use crate::errors::Result;

pub struct Fingerprinter {
    use_content_hash: bool,
    quick_mode: bool,
}

impl Fingerprinter {
    pub fn new(use_content_hash: bool) -> Self {
        Self {
            use_content_hash,
            quick_mode: !use_content_hash,
        }
    }
    
    pub fn fingerprint_file(&self, path: &Path) -> Result<FileFingerprint> {
        let metadata = path.metadata()?;
        
        let hash = if self.use_content_hash {
            self.compute_content_hash(path)?
        } else {
            self.compute_quick_hash(&metadata)?
        };
        
        Ok(FileFingerprint {
            path: path.to_path_buf(),
            modified: metadata.modified()?,
            size: metadata.len(),
            hash,
        })
    }
    
    fn compute_content_hash(&self, path: &Path) -> Result<String> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut hasher = Sha256::new();
        
        // Read in chunks for large files
        let mut buffer = [0; 8192];
        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }
    
    fn compute_quick_hash(&self, metadata: &Metadata) -> Result<String> {
        // Quick hash based on metadata only
        let modified = metadata.modified()?
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();
        let size = metadata.len();
        
        Ok(format!("{}-{}", modified, size))
    }
    
    pub fn has_file_changed(
        &self,
        path: &Path,
        cached_state: &FileState,
    ) -> Result<bool> {
        let current = self.fingerprint_file(path)?;
        
        // Quick check: size or modification time
        if current.size != cached_state.size {
            return Ok(true);
        }
        
        if self.quick_mode {
            // Just check modification time
            Ok(current.modified != cached_state.modified)
        } else {
            // Full content hash comparison
            Ok(current.hash != cached_state.hash)
        }
    }
}

pub struct FileFingerprint {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
    pub hash: String,
}
```

### Step 1: Update Scanner for Incremental Support (2 hours)
```rust
// src/scanner.rs
use crate::state_manager::{StateManager, ScanState, FileState, CachedMatch};
use crate::fingerprint::Fingerprinter;

pub struct IncrementalScanner {
    scanner: Scanner,
    state_manager: StateManager,
    fingerprinter: Fingerprinter,
}

impl IncrementalScanner {
    pub fn new(
        patterns: Vec<Pattern>,
        project_root: &Path,
        use_content_hash: bool,
    ) -> Result<Self> {
        let scanner = Scanner::new(patterns)?;
        let state_manager = StateManager::new(project_root)?;
        let fingerprinter = Fingerprinter::new(use_content_hash);
        
        Ok(Self {
            scanner,
            state_manager,
            fingerprinter,
        })
    }
    
    pub fn scan_incremental(
        &self,
        dir: &Path,
        extensions: &[String],
        force_full: bool,
    ) -> Result<Vec<Match>> {
        // Load previous state
        let mut state = if force_full {
            None
        } else {
            self.state_manager.load_state()?
        };
        
        // Check if patterns changed
        let current_patterns_hash = self.compute_patterns_hash()?;
        
        if let Some(ref s) = state {
            if s.patterns_hash != current_patterns_hash {
                println!("Patterns changed, performing full scan");
                state = None;
            }
        }
        
        let mut all_matches = Vec::new();
        let mut new_state = ScanState {
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_scan: SystemTime::now(),
            files: HashMap::new(),
            patterns_hash: current_patterns_hash,
            scan_results: HashMap::new(),
        };
        
        // Collect all files
        let files = self.collect_files(dir, extensions)?;
        
        // Statistics
        let mut scanned = 0;
        let mut cached = 0;
        let mut skipped = 0;
        
        for file_path in files {
            let needs_scan = if let Some(ref prev_state) = state {
                if let Some(cached_file) = prev_state.files.get(&file_path) {
                    // Check if file changed
                    self.fingerprinter.has_file_changed(&file_path, cached_file)?
                } else {
                    // New file
                    true
                }
            } else {
                // No previous state
                true
            };
            
            if needs_scan {
                // Scan the file
                let matches = self.scanner.scan_file(&file_path)?;
                
                // Update state
                let file_state = FileState {
                    path: file_path.clone(),
                    modified: file_path.metadata()?.modified()?,
                    size: file_path.metadata()?.len(),
                    hash: self.fingerprinter.fingerprint_file(&file_path)?.hash,
                    last_scanned: SystemTime::now(),
                };
                
                new_state.files.insert(file_path.clone(), file_state);
                
                // Cache results
                let cached_matches: Vec<CachedMatch> = matches.iter()
                    .map(|m| CachedMatch {
                        pattern_name: m.pattern_name.clone(),
                        line_number: m.line_number,
                        line_content: m.line_content.clone(),
                    })
                    .collect();
                
                new_state.scan_results.insert(file_path.clone(), cached_matches);
                all_matches.extend(matches);
                scanned += 1;
            } else {
                // Use cached results
                if let Some(ref prev_state) = state {
                    if let Some(cached_matches) = prev_state.scan_results.get(&file_path) {
                        // Convert cached matches back to Match objects
                        let matches: Vec<Match> = cached_matches.iter()
                            .map(|cm| Match {
                                pattern_name: cm.pattern_name.clone(),
                                file_path: file_path.clone(),
                                line_number: cm.line_number,
                                line_content: cm.line_content.clone(),
                            })
                            .collect();
                        
                        all_matches.extend(matches);
                        
                        // Copy file state
                        if let Some(file_state) = prev_state.files.get(&file_path) {
                            new_state.files.insert(file_path.clone(), file_state.clone());
                        }
                        
                        // Copy cached results
                        new_state.scan_results.insert(file_path.clone(), cached_matches.clone());
                        
                        cached += 1;
                    }
                } else {
                    skipped += 1;
                }
            }
        }
        
        // Save new state
        self.state_manager.save_state(&new_state)?;
        
        println!("Incremental scan complete: {} scanned, {} cached, {} skipped",
            scanned, cached, skipped);
        
        Ok(all_matches)
    }
    
    fn compute_patterns_hash(&self) -> Result<String> {
        use sha2::{Sha256, Digest};
        
        let mut hasher = Sha256::new();
        
        for (name, regex) in &self.scanner.patterns {
            hasher.update(name.as_bytes());
            hasher.update(regex.as_str().as_bytes());
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }
}
```

### Step 2: Add CLI Support (30 min)
```rust
// src/cli.rs
#[derive(Subcommand, Debug)]
pub enum Commands {
    Scan {
        // ... existing fields ...
        
        /// Enable incremental scanning (cache results)
        #[arg(long = "incremental", short = 'i')]
        incremental: bool,
        
        /// Force full scan (ignore cache)
        #[arg(long = "force-full")]
        force_full: bool,
        
        /// Clear cache before scanning
        #[arg(long = "clear-cache")]
        clear_cache: bool,
        
        /// Use content hash instead of timestamp
        #[arg(long = "content-hash")]
        content_hash: bool,
    },
}
```

### Step 3: Update Cargo.toml (15 min)
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sha2 = "0.10"
dirs = "5.0"
```

## Test Requirements

### Unit Tests
```rust
// src/state_manager.rs - tests module
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let manager = StateManager::new(temp_dir.path()).unwrap();
        
        let mut state = ScanState {
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_scan: SystemTime::now(),
            files: HashMap::new(),
            patterns_hash: "test_hash".to_string(),
            scan_results: HashMap::new(),
        };
        
        // Add test data
        state.files.insert(
            PathBuf::from("test.txt"),
            FileState {
                path: PathBuf::from("test.txt"),
                modified: SystemTime::now(),
                size: 100,
                hash: "abc123".to_string(),
                last_scanned: SystemTime::now(),
            }
        );
        
        // Save and load
        manager.save_state(&state).unwrap();
        let loaded = manager.load_state().unwrap().unwrap();
        
        assert_eq!(loaded.patterns_hash, state.patterns_hash);
        assert_eq!(loaded.files.len(), 1);
    }
    
    #[test]
    fn test_version_invalidation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = StateManager::new(temp_dir.path()).unwrap();
        
        let state = ScanState {
            version: "0.0.1".to_string(), // Old version
            // ... other fields
        };
        
        manager.save_state(&state).unwrap();
        let loaded = manager.load_state().unwrap();
        
        // Should invalidate due to version mismatch
        assert!(loaded.is_none());
    }
}

// src/fingerprint.rs - tests module
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;
    
    #[test]
    fn test_content_hash_changes() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"Hello, World!").unwrap();
        
        let fingerprinter = Fingerprinter::new(true);
        let fp1 = fingerprinter.fingerprint_file(file.path()).unwrap();
        
        // Modify file
        file.write_all(b" Modified").unwrap();
        let fp2 = fingerprinter.fingerprint_file(file.path()).unwrap();
        
        assert_ne!(fp1.hash, fp2.hash);
    }
    
    #[test]
    fn test_quick_mode_performance() {
        use std::time::Instant;
        
        let file = create_large_test_file(10_000_000); // 10MB
        
        let quick = Fingerprinter::new(false);
        let full = Fingerprinter::new(true);
        
        let start = Instant::now();
        quick.fingerprint_file(&file).unwrap();
        let quick_time = start.elapsed();
        
        let start = Instant::now();
        full.fingerprint_file(&file).unwrap();
        let full_time = start.elapsed();
        
        // Quick mode should be >10x faster
        assert!(quick_time < full_time / 10);
    }
}
```

### Integration Tests
```rust
// tests/incremental_integration.rs
#[test]
fn test_incremental_speedup() {
    let temp_dir = create_test_project(1000); // 1000 files
    
    // First scan (cold)
    let start = Instant::now();
    let scanner = IncrementalScanner::new(patterns, temp_dir.path(), false).unwrap();
    let results1 = scanner.scan_incremental(temp_dir.path(), &[], false).unwrap();
    let cold_time = start.elapsed();
    
    // Second scan (warm, no changes)
    let start = Instant::now();
    let results2 = scanner.scan_incremental(temp_dir.path(), &[], false).unwrap();
    let warm_time = start.elapsed();
    
    // Should be >80% faster
    assert!(warm_time < cold_time / 5);
    assert_eq!(results1.len(), results2.len());
}

#[test]
fn test_file_modification_detection() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, "initial content").unwrap();
    
    let scanner = IncrementalScanner::new(patterns, temp_dir.path(), false).unwrap();
    
    // First scan
    let results1 = scanner.scan_incremental(temp_dir.path(), &[], false).unwrap();
    
    // Modify file
    thread::sleep(Duration::from_millis(10));
    fs::write(&file_path, "modified content with pattern").unwrap();
    
    // Second scan should detect change
    let results2 = scanner.scan_incremental(temp_dir.path(), &[], false).unwrap();
    
    assert_ne!(results1.len(), results2.len());
}
```

### Edge Cases to Test
- Corrupted cache file
- Cache directory permissions
- Files deleted between scans
- Files renamed between scans
- Symbolic links
- Pattern changes
- Concurrent scans
- Cache size limits
- Clock skew issues

## Definition of Done

### Code Complete
- [x] StateManager implemented with atomic writes
- [x] Fingerprinting with quick and full modes
- [x] Incremental scanner integration
- [x] CLI flags for control
- [x] Cache invalidation logic

### Testing Complete
- [x] Unit tests >90% coverage
- [x] Integration tests verify speedup
- [x] Cache persistence tested
- [x] File change detection accurate
- [x] Cross-platform cache locations

### Documentation Complete
- [x] Cache format documented
- [x] Performance metrics documented
- [x] CLI help updated

## Time Estimate: 6 hours

| Task | Duration | Notes |
|------|----------|-------|
| StateManager module | 1.5h | Persistence logic |
| Fingerprinting module | 1h | Hash computation |
| Incremental scanner | 2h | Core integration |
| CLI integration | 0.5h | Flags and options |
| Unit tests | 0.5h | State verification |
| Integration tests | 0.5h | Performance validation |

**Buffer:** +1h for cache debugging
**Total:** 7h (1 day)

## Performance Metrics
- **First Scan:** Baseline performance (no regression)
- **Subsequent Scans:** 80-90% faster for unchanged files
- **Cache Size:** ~1KB per file
- **Hash Computation:** <1ms quick mode, <10ms full mode per file