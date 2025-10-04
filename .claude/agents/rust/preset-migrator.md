# Preset Migrator Agent  

## Mission
Extract 170 lines of hardcoded preset patterns from main.rs into maintainable YAML configurations.

## Prerequisites
- Can work independently from other agents
- Creates presets/ directory structure

## Current Problem

The `get_preset_config` function (main.rs:553-723) is a maintenance nightmare:
- 170 lines of hardcoded patterns
- Copy-paste everywhere
- No way for users to customize presets
- Broken pattern at line 692 (empty end marker)

## Implementation Plan

### 1. Create Preset Directory Structure

```
presets/
├── clean-debug.yaml
├── remove-copyright.yaml
├── remove-todos.yaml
├── trim-whitespace.yaml
├── remove-empty-comments.yaml
├── tabs-to-spaces.yaml
├── spaces-to-tabs.yaml
└── user/           # For user custom presets
    └── .gitkeep
```

### 2. Convert Hardcoded Presets to YAML

#### presets/remove-copyright.yaml
```yaml
name: remove-copyright
description: Remove copyright headers and license blocks
patterns: []
replacements: []
blocks:
  # Python style copyright
  - start: "# Copyright (c)"
    end: "# DEALINGS IN THE SOFTWARE."
  
  # C++ style copyright  
  - start: "// Copyright (c)"
    end: "// DEALINGS IN THE SOFTWARE."
  
  # C style copyright
  - start: "/* Copyright (c)"
    end: "*/"
  
  # Javadoc style copyright
  - start: "/**\n * Copyright (c)"
    end: " */"
  
  # MIT License blocks
  - start: "# MIT License"
    end: "# SOFTWARE."
  
  - start: "// MIT License"
    end: "// SOFTWARE."
  
  # Apache License blocks
  - start: "# Licensed under the Apache License"
    end: "# limitations under the License."
    
  - start: "// Licensed under the Apache License"
    end: "// limitations under the License."

extensions: null  # Apply to all files
exclude: 
  - LICENSE
  - LICENSE.md
  - LICENSE.txt
  - COPYING
```

#### presets/clean-debug.yaml
```yaml
name: clean-debug
description: Clean up debug code and logging statements
patterns:
  - pattern: 'console\.log\([^)]*\)'
    replacement: '// console.log()'
    
  - pattern: 'console\.debug\([^)]*\)'
    replacement: '// console.debug()'
    
  - pattern: 'console\.trace\([^)]*\)'
    replacement: '// console.trace()'
    
  - pattern: 'debugger\s*;?'
    replacement: '// debugger;'
    
  - pattern: 'print\([^)]*\)\s*#\s*DEBUG'
    replacement: '# print() # DEBUG'
    
  - pattern: 'print\([^)]*\)\s*#\s*debug'
    replacement: '# print() # debug'
    
  - pattern: 'logger\.debug\([^)]*\)'
    replacement: '// logger.debug()'
    
  - pattern: 'Debug\.Print\([^)]*\)'
    replacement: '// Debug.Print()'
    
  - pattern: 'System\.out\.println\([^)]*\);\s*//\s*DEBUG'
    replacement: '// System.out.println(); // DEBUG'

blocks:
  - start: "// DEBUG START"
    end: "// DEBUG END"
    
  - start: "/* DEBUG START */"
    end: "/* DEBUG END */"
    
  - start: "# DEBUG START"
    end: "# DEBUG END"

extensions:
  - js
  - ts
  - jsx
  - tsx
  - py
  - java
  - cs
  - go
  - rs

exclude:
  - node_modules
  - vendor
  - .git
```

#### presets/remove-todos.yaml
```yaml
name: remove-todos
description: Remove TODO, FIXME, HACK, and XXX comments
patterns:
  # JavaScript/C style comments
  - pattern: '.*//\s*TODO:?.*'
    replacement: ''
    
  - pattern: '.*//\s*FIXME:?.*'
    replacement: ''
    
  - pattern: '.*//\s*HACK:?.*'
    replacement: ''
    
  - pattern: '.*//\s*XXX:?.*'
    replacement: ''
  
  # Python/Shell style comments
  - pattern: '.*#\s*TODO:?.*'
    replacement: ''
    
  - pattern: '.*#\s*FIXME:?.*'
    replacement: ''
    
  - pattern: '.*#\s*HACK:?.*'
    replacement: ''
    
  - pattern: '.*#\s*XXX:?.*'
    replacement: ''

blocks: []
extensions: null  # All files
exclude: 
  - TODO.md
  - CHANGELOG.md
```

#### presets/trim-whitespace.yaml
```yaml
name: trim-whitespace
description: Remove trailing whitespace from lines
patterns:
  - pattern: '[ \t]+$'
    replacement: ''

blocks: []
extensions: null  # All text files
exclude:
  - .git
  - node_modules
  - "*.min.js"
  - "*.min.css"
```

#### presets/remove-empty-comments.yaml
```yaml
name: remove-empty-comments  
description: Remove empty comment lines
patterns:
  - pattern: '^\s*//\s*$'
    replacement: ''
    
  - pattern: '^\s*#\s*$'
    replacement: ''
    
  - pattern: '^\s*/\*\s*\*/\s*$'
    replacement: ''

blocks:
  # Note: Multi-line empty comment blocks need special handling
  - start: "/*"
    end: "*/"
    condition: "empty"  # New feature: only remove if block is empty

extensions: null
exclude: []
```

#### presets/tabs-to-spaces.yaml
```yaml
name: tabs-to-spaces
description: Convert tabs to 4 spaces
patterns:
  - pattern: '\t'
    replacement: '    '

blocks: []
extensions:
  - py      # Python requires spaces
  - yaml
  - yml
  - md
exclude: 
  - Makefile  # Makefiles require tabs
  - "*.mk"
```

#### presets/spaces-to-tabs.yaml
```yaml
name: spaces-to-tabs
description: Convert 4 spaces to tabs
patterns:
  - pattern: '    '  # 4 spaces
    replacement: '\t'

blocks: []
extensions:
  - go      # Go prefers tabs
  - c
  - cpp
  - h
exclude: 
  - "*.yaml"
  - "*.yml"
  - "*.py"
```

### 3. Create Preset Loader (src/presets/loader.rs)

```rust
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct PresetConfig {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub patterns: Vec<PatternRule>,
    #[serde(default)]
    pub blocks: Vec<BlockRule>,
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PatternRule {
    pub pattern: String,
    pub replacement: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockRule {
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub condition: Option<String>, // "empty", "contains:xyz", etc.
}

pub struct PresetLoader {
    preset_dirs: Vec<PathBuf>,
}

impl PresetLoader {
    pub fn new() -> Self {
        let mut dirs = vec![];
        
        // Built-in presets (shipped with binary)
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                dirs.push(exe_dir.join("presets"));
                
                // Development location
                if let Some(parent) = exe_dir.parent() {
                    if let Some(grandparent) = parent.parent() {
                        dirs.push(grandparent.join("presets"));
                    }
                }
            }
        }
        
        // User presets
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".uber_scanner/presets"));
        }
        
        // Current directory presets
        dirs.push(PathBuf::from("./presets"));
        
        Self { preset_dirs: dirs }
    }
    
    pub fn load(&self, name: &str) -> Result<PresetConfig> {
        // Try each directory
        for dir in &self.preset_dirs {
            let preset_file = dir.join(format!("{}.yaml", name));
            if preset_file.exists() {
                return self.load_file(&preset_file);
            }
            
            // Also check user subdirectory
            let user_preset = dir.join("user").join(format!("{}.yaml", name));
            if user_preset.exists() {
                return self.load_file(&user_preset);
            }
        }
        
        Err(PresetError::NotFound {
            name: name.to_string(),
            searched_dirs: self.preset_dirs.clone(),
        })
    }
    
    fn load_file(&self, path: &Path) -> Result<PresetConfig> {
        let content = fs::read_to_string(path)
            .map_err(|e| PresetError::ReadFailed {
                path: path.to_path_buf(),
                error: e,
            })?;
        
        let preset: PresetConfig = serde_yaml::from_str(&content)
            .map_err(|e| PresetError::ParseFailed {
                path: path.to_path_buf(),
                error: e,
            })?;
        
        // Validate preset
        self.validate(&preset)?;
        
        Ok(preset)
    }
    
    fn validate(&self, preset: &PresetConfig) -> Result<()> {
        // Validate patterns compile
        for rule in &preset.patterns {
            Regex::new(&rule.pattern)
                .map_err(|e| PresetError::InvalidPattern {
                    preset: preset.name.clone(),
                    pattern: rule.pattern.clone(),
                    error: e,
                })?;
        }
        
        // Validate blocks
        for block in &preset.blocks {
            if block.start.is_empty() {
                return Err(PresetError::InvalidBlock {
                    preset: preset.name.clone(),
                    message: "Block start cannot be empty".into(),
                });
            }
            
            // Note: empty end is valid for some patterns
        }
        
        Ok(())
    }
    
    pub fn list_available(&self) -> Vec<PresetInfo> {
        let mut presets = Vec::new();
        let mut seen = std::collections::HashSet::new();
        
        for dir in &self.preset_dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.path().file_stem() {
                        let name = name.to_string_lossy().to_string();
                        if !seen.contains(&name) && entry.path().extension() == Some("yaml".as_ref()) {
                            seen.insert(name.clone());
                            
                            // Try to load description
                            if let Ok(preset) = self.load(&name) {
                                presets.push(PresetInfo {
                                    name: preset.name,
                                    description: preset.description,
                                    path: entry.path(),
                                });
                            }
                        }
                    }
                }
            }
        }
        
        presets.sort_by(|a, b| a.name.cmp(&b.name));
        presets
    }
}

pub struct PresetInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}
```

### 4. Update CLI to Support Preset Discovery

```rust
// In cli.rs
impl Args {
    pub fn list_presets() {
        let loader = PresetLoader::new();
        let presets = loader.list_available();
        
        println!("Available presets:\n");
        for preset in presets {
            println!("  {} - {}", preset.name, preset.description);
            println!("    Location: {}", preset.path.display());
        }
        
        println!("\nUse: uber_scanner replace --preset <name>");
        println!("Create custom presets in ~/.uber_scanner/presets/");
    }
}
```

### 5. Migration Script

```bash
#!/bin/bash
# migrate-presets.sh

echo "Creating preset directory structure..."
mkdir -p presets/user

echo "Generating preset YAML files..."

# Generate each preset file
cat > presets/clean-debug.yaml << 'EOF'
# ... preset content ...
EOF

# Continue for all presets...

echo "Validating presets..."
for preset in presets/*.yaml; do
    echo -n "  Validating $(basename $preset)... "
    if cargo run -- validate-preset "$preset" 2>/dev/null; then
        echo "✓"
    else
        echo "✗ FAILED"
        exit 1
    fi
done

echo "Migration complete!"
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_load_builtin_preset() {
        let loader = PresetLoader::new();
        let preset = loader.load("clean-debug").unwrap();
        
        assert_eq!(preset.name, "clean-debug");
        assert!(!preset.patterns.is_empty());
    }
    
    #[test]
    fn test_user_preset_override() {
        // Create user preset that overrides built-in
        let user_dir = dirs::home_dir().unwrap()
            .join(".uber_scanner/presets");
        fs::create_dir_all(&user_dir).unwrap();
        
        // User preset should take precedence
    }
    
    #[test]
    fn test_invalid_preset_fails() {
        let loader = PresetLoader::new();
        let result = loader.load("nonexistent");
        
        assert!(result.is_err());
        match result.unwrap_err() {
            PresetError::NotFound { name, .. } => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Wrong error type"),
        }
    }
    
    #[test]
    fn test_preset_with_invalid_regex() {
        // Test that presets with invalid patterns fail validation
    }
}
```

## Success Criteria

- All hardcoded presets extracted to YAML files
- `get_preset_config` function deleted
- Users can add custom presets to ~/.uber_scanner/presets/
- Preset validation at load time
- `--list-presets` command shows available presets
- All existing preset functionality preserved
- Preset files are human-readable and editable