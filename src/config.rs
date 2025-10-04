use crate::errors::Result;
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Represents a named pattern used for scanning.
#[derive(Deserialize)]
pub struct Pattern {
    /// The name of the pattern.
    pub name: String,
    /// The regex pattern string.
    pub pattern: String,
}

/// Configuration for the scan operation.
#[derive(Deserialize)]
pub struct ScanConfig {
    /// A list of patterns to scan for.
    pub patterns: Vec<Pattern>,
}

/// Configuration for the replace operation.
#[derive(Deserialize, Clone)]
pub struct ReplaceConfig {
    /// A list of regex patterns to search for.
    pub patterns: Vec<String>,
    /// A list of replacement strings. Each element corresponds to a pattern.
    /// `None` can be used to indicate no replacement for a given pattern.
    pub replacements: Vec<Option<String>>,
    /// A list of blocks to ignore during replacement.
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// An optional list of file extensions to include in the operation.
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    /// An optional list of file or directory paths to exclude from the operation.
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

/// Defines a block of text to be ignored, specified by start and end patterns.
#[derive(Deserialize, Clone)]
pub struct Block {
    /// The pattern that marks the start of a block.
    pub start: String,
    /// The pattern that marks the end of a block.
    pub end: String,
}

/// A utility for loading scanner and replacer configurations.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Finds the configuration file by searching in a prioritized list of locations.
    ///
    /// The search order is:
    /// 1. The absolute path provided in `config_path`, if it exists.
    /// 2. A path relative to the current directory.
    /// 3. A path relative to the `working_dir`.
    /// 4. Inside the `~/.uber_scanner` directory.
    /// 5. Next to the executable.
    /// 6. In the parent directory of the executable (to handle `target/release` builds).
    /// 7. In the grandparent directory of the executable.
    ///
    /// # Arguments
    ///
    /// * `config_path` - The path to the configuration file, which can be absolute or relative.
    /// * `working_dir` - The directory where the command is being executed.
    ///
    /// # Returns
    ///
    /// Returns a `Result` with the `PathBuf` to the found config file, or an error if not found.
    pub fn find_config(config_path: &Path, working_dir: &Path) -> Result<PathBuf> {
        // If the path is absolute and exists, use it
        if config_path.is_absolute() && config_path.exists() {
            return Ok(config_path.to_path_buf());
        }

        // Try relative to current directory
        if config_path.exists() {
            return Ok(config_path.to_path_buf());
        }

        // Try relative to the working directory
        let in_working_dir = working_dir.join(config_path);
        if in_working_dir.exists() {
            return Ok(in_working_dir);
        }

        // Try in .uber_scanner config directory
        if let Some(home) = env::var_os("HOME") {
            let home_config = PathBuf::from(home).join(".uber_scanner").join(config_path);
            if home_config.exists() {
                return Ok(home_config);
            }
        }

        // Try in the executable's directory
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let exe_config = exe_dir.join(config_path);
                if exe_config.exists() {
                    return Ok(exe_config);
                }

                // Also check parent directory (in case we're in target/release)
                if let Some(parent) = exe_dir.parent() {
                    let parent_config = parent.join(config_path);
                    if parent_config.exists() {
                        return Ok(parent_config);
                    }

                    // Check one more level up (for target/release structure)
                    if let Some(grandparent) = parent.parent() {
                        let grandparent_config = grandparent.join(config_path);
                        if grandparent_config.exists() {
                            return Ok(grandparent_config);
                        }
                    }
                }
            }
        }

        // If we still haven't found it, provide a helpful error
        let mut tried_locations = vec![
            config_path.display().to_string(),
            in_working_dir.display().to_string(),
        ];

        if let Some(home) = env::var_os("HOME") {
            tried_locations.push(
                PathBuf::from(home)
                    .join(".uber_scanner")
                    .join(config_path)
                    .display()
                    .to_string(),
            );
        }

        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                tried_locations.push(exe_dir.join(config_path).display().to_string());
            }
        }

        Err(format!(
            "Config file '{}' not found. Searched in:\n  - {}",
            config_path.display(),
            tried_locations.join("\n  - ")
        )
        .into())
    }

    /// Loads a `ScanConfig` from a YAML file.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the YAML configuration file.
    pub fn load_scan_config(path: &Path) -> Result<ScanConfig> {
        let file = File::open(path)?;
        Ok(serde_yaml::from_reader(file)?)
    }

    /// Loads a `ReplaceConfig` from a YAML file.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the YAML configuration file.
    pub fn load_replace_config(path: &Path) -> Result<ReplaceConfig> {
        let file = File::open(path)?;
        Ok(serde_yaml::from_reader(file)?)
    }
}
