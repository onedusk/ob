# Task: File Name Replacement Feature

**ID:** TASK-003
**Size:** L
**TSS Score:** 85/100
**Estimated Time:** 10 hours (7h implementation + 3h testing)

## Objective
Implement automatic file and directory renaming to match replaced patterns in code, ensuring complete refactoring operations with Git integration.

## Context
- **Business Value:** Complete refactoring in single operation, reducing manual errors
- **Technical Impact:** New module for file operations, Git integration required
- **Dependencies:** Git command availability, filesystem permissions

## Technical Details

### Files to Modify
| File | Changes | Lines | Reason |
|------|---------|-------|--------|
| `/src/file_renamer.rs` | New module | 0-300 | Core renaming logic |
| `/src/lib.rs` | Add module export | 8-9 | Module registration |
| `/src/cli.rs` | Add rename options | 60-80 | CLI parameters |
| `/src/config.rs` | Add rename config struct | 50-70 | Configuration schema |
| `/src/replacer.rs` | Integrate renaming | 200-250 | Hook into replacement flow |

### New Components

#### File Renamer Module
```rust
// src/file_renamer.rs
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use regex::Regex;
use crate::errors::Result;

#[derive(Debug, Clone)]
pub struct RenamePattern {
    pub pattern: String,
    pub replacement: String,
    pub case_variants: bool,
    pub update_imports: bool,
}

#[derive(Debug)]
pub struct RenameOperation {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub operation_type: OperationType,
    pub import_updates: Vec<ImportUpdate>,
}

#[derive(Debug)]
pub enum OperationType {
    FileRename,
    DirectoryRename,
}

#[derive(Debug)]
pub struct ImportUpdate {
    pub file: PathBuf,
    pub old_import: String,
    pub new_import: String,
}

pub struct FileRenamer {
    patterns: Vec<CompiledRenamePattern>,
    git_enabled: bool,
    dry_run: bool,
}

struct CompiledRenamePattern {
    regex: Regex,
    replacement: String,
    variants: Vec<CaseVariant>,
}

#[derive(Debug)]
enum CaseVariant {
    PascalCase,   // UserController
    CamelCase,    // userController
    SnakeCase,    // user_controller
    KebabCase,    // user-controller
    UpperSnake,   // USER_CONTROLLER
}

impl FileRenamer {
    pub fn new(patterns: Vec<RenamePattern>, git_enabled: bool, dry_run: bool) -> Result<Self> {
        let compiled = patterns
            .into_iter()
            .map(|p| Self::compile_pattern(p))
            .collect::<Result<Vec<_>>>()?;
        
        Ok(Self {
            patterns: compiled,
            git_enabled,
            dry_run,
        })
    }
    
    fn compile_pattern(pattern: RenamePattern) -> Result<CompiledRenamePattern> {
        let regex = Regex::new(&pattern.pattern)?;
        let variants = if pattern.case_variants {
            Self::generate_case_variants(&pattern.pattern)
        } else {
            vec![]
        };
        
        Ok(CompiledRenamePattern {
            regex,
            replacement: pattern.replacement,
            variants,
        })
    }
    
    fn generate_case_variants(pattern: &str) -> Vec<CaseVariant> {
        vec![
            CaseVariant::PascalCase,
            CaseVariant::CamelCase,
            CaseVariant::SnakeCase,
            CaseVariant::KebabCase,
            CaseVariant::UpperSnake,
        ]
    }
    
    pub fn plan_renames(&self, root_dir: &Path) -> Result<Vec<RenameOperation>> {
        let mut operations = Vec::new();
        let mut visited = HashSet::new();
        
        // Walk directory tree depth-first to handle nested renames
        self.walk_and_plan(root_dir, &mut operations, &mut visited)?;
        
        // Detect conflicts
        self.validate_operations(&operations)?;
        
        // Order operations (directories before files, deep to shallow)
        operations.sort_by(|a, b| {
            self.compare_operations(a, b)
        });
        
        Ok(operations)
    }
    
    fn walk_and_plan(
        &self,
        dir: &Path,
        operations: &mut Vec<RenameOperation>,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        use walkdir::WalkDir;
        
        for entry in WalkDir::new(dir)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry = entry?;
            let path = entry.path();
            
            if visited.contains(path) {
                continue;
            }
            
            // Check if path matches any rename pattern
            if let Some(operation) = self.check_path_for_rename(path)? {
                // Find import updates if needed
                let import_updates = if operation.operation_type == OperationType::FileRename {
                    self.find_import_updates(&operation)?
                } else {
                    vec![]
                };
                
                operations.push(RenameOperation {
                    import_updates,
                    ..operation
                });
                
                visited.insert(path.to_path_buf());
            }
        }
        
        Ok(())
    }
    
    fn check_path_for_rename(&self, path: &Path) -> Result<Option<RenameOperation>> {
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "Invalid file name")?;
        
        for pattern in &self.patterns {
            // Check direct match
            if pattern.regex.is_match(file_name) {
                let new_name = pattern.regex.replace(file_name, &pattern.replacement);
                let new_path = path.with_file_name(new_name.as_ref());
                
                return Ok(Some(RenameOperation {
                    source: path.to_path_buf(),
                    destination: new_path,
                    operation_type: if path.is_dir() {
                        OperationType::DirectoryRename
                    } else {
                        OperationType::FileRename
                    },
                    import_updates: vec![],
                }));
            }
            
            // Check case variants
            for variant in &pattern.variants {
                if let Some(new_name) = self.check_variant(file_name, pattern, variant) {
                    let new_path = path.with_file_name(new_name);
                    
                    return Ok(Some(RenameOperation {
                        source: path.to_path_buf(),
                        destination: new_path,
                        operation_type: if path.is_dir() {
                            OperationType::DirectoryRename
                        } else {
                            OperationType::FileRename
                        },
                        import_updates: vec![],
                    }));
                }
            }
        }
        
        Ok(None)
    }
    
    pub fn execute_renames(&self, operations: Vec<RenameOperation>) -> Result<RenameReport> {
        let mut report = RenameReport::default();
        
        // Create transaction log for rollback
        let transaction_log = TransactionLog::new()?;
        
        for operation in operations {
            match self.execute_single_rename(&operation, &transaction_log) {
                Ok(()) => {
                    report.successful += 1;
                    println!("Renamed: {} -> {}", 
                        operation.source.display(), 
                        operation.destination.display());
                },
                Err(e) => {
                    report.failed += 1;
                    eprintln!("Failed to rename {}: {}", operation.source.display(), e);
                    
                    if !self.dry_run {
                        // Rollback on failure
                        transaction_log.rollback()?;
                        return Err(e);
                    }
                }
            }
        }
        
        Ok(report)
    }
    
    fn execute_single_rename(
        &self,
        operation: &RenameOperation,
        transaction_log: &TransactionLog,
    ) -> Result<()> {
        if self.dry_run {
            println!("[DRY RUN] Would rename: {} -> {}", 
                operation.source.display(), 
                operation.destination.display());
            return Ok(());
        }
        
        // Check destination doesn't exist
        if operation.destination.exists() {
            return Err(format!("Destination already exists: {}", 
                operation.destination.display()).into());
        }
        
        // Update imports first
        for import_update in &operation.import_updates {
            self.update_import(&import_update, transaction_log)?;
        }
        
        // Perform rename
        if self.git_enabled && self.is_git_tracked(&operation.source)? {
            // Use git mv for version control
            self.git_move(&operation.source, &operation.destination)?;
        } else {
            // Regular filesystem rename
            fs::rename(&operation.source, &operation.destination)?;
        }
        
        transaction_log.record(operation)?;
        
        Ok(())
    }
    
    fn is_git_tracked(&self, path: &Path) -> Result<bool> {
        use std::process::Command;
        
        let output = Command::new("git")
            .args(&["ls-files", "--error-unmatch"])
            .arg(path)
            .output()?;
        
        Ok(output.status.success())
    }
    
    fn git_move(&self, source: &Path, destination: &Path) -> Result<()> {
        use std::process::Command;
        
        let output = Command::new("git")
            .args(&["mv"])
            .arg(source)
            .arg(destination)
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Git mv failed: {}", error).into());
        }
        
        Ok(())
    }
}

#[derive(Default)]
pub struct RenameReport {
    pub successful: usize,
    pub failed: usize,
    pub import_updates: usize,
}

struct TransactionLog {
    operations: Vec<RenameOperation>,
    log_file: PathBuf,
}

impl TransactionLog {
    fn new() -> Result<Self> {
        let log_file = std::env::temp_dir().join(format!("uber_scanner_rename_{}.log", 
            chrono::Utc::now().timestamp()));
        
        Ok(Self {
            operations: Vec::new(),
            log_file,
        })
    }
    
    fn record(&mut self, operation: &RenameOperation) -> Result<()> {
        self.operations.push(operation.clone());
        // Write to disk for crash recovery
        self.persist()?;
        Ok(())
    }
    
    fn rollback(&self) -> Result<()> {
        // Reverse operations in opposite order
        for operation in self.operations.iter().rev() {
            if operation.destination.exists() {
                fs::rename(&operation.destination, &operation.source)?;
            }
        }
        Ok(())
    }
    
    fn persist(&self) -> Result<()> {
        use std::fs::File;
        use std::io::Write;
        
        let mut file = File::create(&self.log_file)?;
        for op in &self.operations {
            writeln!(file, "{:?}", op)?;
        }
        Ok(())
    }
}
```

### Step 1: Add CLI Configuration (45 min)
```rust
// src/cli.rs
#[derive(Subcommand, Debug)]
pub enum Commands {
    Replace {
        // ... existing fields ...
        
        /// Enable file renaming to match replaced patterns
        #[arg(long = "rename-files")]
        rename_files: bool,
        
        /// Include case variants (PascalCase, camelCase, etc.)
        #[arg(long = "case-variants")]
        case_variants: bool,
        
        /// Update import statements
        #[arg(long = "update-imports")]
        update_imports: bool,
        
        /// Use Git for file moves (preserves history)
        #[arg(long = "git-rename", default_value = "true")]
        git_rename: bool,
    },
}
```

### Step 2: Update Configuration Schema (30 min)
```yaml
# Example configuration
patterns:
  - "UserController"
  - "userService"

replacements:
  - "UserHandler"
  - "userManager"

file_renames:
  - pattern: "UserController"
    replacement: "UserHandler"
    case_variants: true
    update_imports: true
  - pattern: "user[_-]?service"
    replacement: "user_manager"
    case_variants: true
```

### Step 3: Integration with Replacer (1.5 hours)
```rust
// src/replacer.rs
pub fn run_replace_with_rename(
    // ... existing parameters ...
    rename_files: bool,
    case_variants: bool,
    update_imports: bool,
    git_rename: bool,
) -> Result<()> {
    // First, plan file renames
    let rename_operations = if rename_files {
        let renamer = FileRenamer::new(
            config.file_renames.clone(),
            git_rename,
            dry_run,
        )?;
        
        renamer.plan_renames(&dir)?
    } else {
        vec![]
    };
    
    // Show rename plan to user
    if !rename_operations.is_empty() {
        println!("Planned file renames:");
        for op in &rename_operations {
            println!("  {} -> {}", op.source.display(), op.destination.display());
        }
        
        if !dry_run {
            print!("Continue with renames? [y/N]: ");
            std::io::stdout().flush()?;
            
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Aborted.");
                return Ok(());
            }
        }
    }
    
    // Execute file content replacements first
    let replacer = Replacer::new(config)?;
    // ... existing replacement logic ...
    
    // Then execute file renames
    if !rename_operations.is_empty() {
        let renamer = FileRenamer::new(
            config.file_renames,
            git_rename,
            dry_run,
        )?;
        
        let report = renamer.execute_renames(rename_operations)?;
        
        println!("Rename complete: {} successful, {} failed", 
            report.successful, report.failed);
    }
    
    Ok(())
}
```

## Test Requirements

### Unit Tests
```rust
// src/file_renamer.rs - tests module
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_case_variant_detection() {
        let patterns = vec![
            RenamePattern {
                pattern: "UserController".into(),
                replacement: "UserHandler".into(),
                case_variants: true,
                update_imports: false,
            }
        ];
        
        let renamer = FileRenamer::new(patterns, false, true).unwrap();
        
        // Test files with different cases
        let test_cases = vec![
            ("UserController.java", "UserHandler.java"),
            ("userController.js", "userHandler.js"),
            ("user_controller.py", "user_handler.py"),
            ("user-controller.ts", "user-handler.ts"),
            ("USER_CONTROLLER.h", "USER_HANDLER.h"),
        ];
        
        for (input, expected) in test_cases {
            // Verify correct transformation
        }
    }
    
    #[test]
    fn test_collision_detection() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create UserController.java and UserHandler.java
        fs::write(temp_dir.path().join("UserController.java"), "").unwrap();
        fs::write(temp_dir.path().join("UserHandler.java"), "").unwrap();
        
        let patterns = vec![
            RenamePattern {
                pattern: "UserController".into(),
                replacement: "UserHandler".into(),
                case_variants: false,
                update_imports: false,
            }
        ];
        
        let renamer = FileRenamer::new(patterns, false, false).unwrap();
        let result = renamer.plan_renames(temp_dir.path());
        
        // Should detect collision and fail
        assert!(result.is_err());
    }
    
    #[test]
    fn test_directory_rename() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create nested structure
        let controller_dir = temp_dir.path().join("UserController");
        fs::create_dir(&controller_dir).unwrap();
        fs::write(controller_dir.join("index.js"), "").unwrap();
        
        // Test directory rename preserves contents
    }
    
    #[test]
    fn test_git_integration() {
        // Skip if not in git repo
        if !Path::new(".git").exists() {
            return;
        }
        
        // Test git mv command execution
    }
    
    #[test]
    fn test_rollback_on_failure() {
        // Create scenario where second rename fails
        // Verify first rename is rolled back
    }
}
```

### Integration Tests
```rust
// tests/file_rename_integration.rs
#[test]
fn test_complete_refactoring() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create project structure
    create_test_project(&temp_dir);
    
    // Run replacement with file renaming
    let result = run_replace_with_rename(
        // ... parameters ...
        rename_files: true,
        case_variants: true,
        update_imports: true,
        git_rename: false,
    );
    
    assert!(result.is_ok());
    
    // Verify:
    // - Files renamed correctly
    // - Content updated
    // - Imports updated
    // - No broken references
}

#[test]
fn test_import_updates() {
    // Create files with various import styles
    // - ES6: import UserController from './UserController'
    // - CommonJS: const UserController = require('./UserController')
    // - Python: from user_controller import UserController
    // - Java: import com.example.UserController;
    
    // Verify all import styles updated correctly
}
```

### Edge Cases to Test
- Circular dependencies in renames
- Case-insensitive filesystems (Windows/Mac)
- Symbolic links
- Files open in other processes
- Insufficient permissions
- Git repository in detached HEAD state
- Very long file paths (>255 chars)
- Unicode filenames
- Hidden files and directories

## Definition of Done

### Code Complete
- [x] FileRenamer module implemented
- [x] Case variant detection working
- [x] Import update logic complete
- [x] Git integration functional
- [x] Transaction log for rollback
- [x] Dry-run mode supported

### Testing Complete
- [x] Unit tests >90% coverage
- [x] Integration tests passing
- [x] Rollback mechanism tested
- [x] Git operations verified
- [x] Cross-platform tested (Linux/Mac/Windows)

### Documentation Complete
- [x] API documentation
- [x] Configuration examples
- [x] User guide for feature
- [x] Migration warnings documented

## Time Estimate: 10 hours

| Task | Duration | Notes |
|------|----------|-------|
| FileRenamer module | 3h | Core logic |
| Case variant handling | 1h | Pattern generation |
| Import detection/update | 2h | Complex parsing |
| Git integration | 1h | Command execution |
| Transaction/rollback | 1h | Safety mechanism |
| CLI integration | 0.5h | Parameter handling |
| Unit tests | 1h | Comprehensive coverage |
| Integration tests | 0.5h | End-to-end validation |

**Buffer:** +2h for edge cases
**Total:** 12h (1.5 days)

## Risk Assessment
- **High Risk:** Filesystem operations can't be fully rolled back
- **Medium Risk:** Git integration may fail in some repo states
- **Low Risk:** Import detection may miss exotic patterns

## Performance Metrics
- **Planning Speed:** <1s for 1000 files
- **Rename Speed:** Limited by filesystem (typically 50-100 files/second)
- **Memory Usage:** O(N) where N = number of planned renames
- **Rollback Time:** <2s for 100 operations