use crate::errors::{Error, Result};
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
    /// `None` (`null` in YAML) deletes the line *after* each line matching the
    /// pattern; the matching line itself is kept.
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
    /// 1. The path as given (absolute, or relative to the current directory).
    /// 2. Relative to `working_dir`.
    /// 3. Inside the user config directory: `$XDG_CONFIG_HOME/oober`, or
    ///    `~/.config/oober` when `XDG_CONFIG_HOME` is unset.
    ///
    /// An absolute path is only tried as given. The executable's directory and
    /// its ancestors are deliberately not searched: a config file planted next
    /// to the binary (or left over in `target/`) must never be picked up
    /// silently.
    ///
    /// # Arguments
    ///
    /// * `config_path` - The path to the configuration file, which can be absolute or relative.
    /// * `working_dir` - The directory the command operates on.
    ///
    /// # Returns
    ///
    /// Returns a `Result` with the `PathBuf` to the found config file, or an error
    /// listing the searched locations if not found.
    pub fn find_config(config_path: &Path, working_dir: &Path) -> Result<PathBuf> {
        Self::find_config_in(config_path, working_dir, Self::user_config_dir().as_deref())
    }

    /// `find_config` with the user config directory injected, so tests can
    /// exercise the search order without touching the environment.
    fn find_config_in(
        config_path: &Path,
        working_dir: &Path,
        user_config_dir: Option<&Path>,
    ) -> Result<PathBuf> {
        // The path as given: absolute, or relative to the current directory.
        if config_path.exists() {
            return Ok(config_path.to_path_buf());
        }

        let mut tried_locations = vec![config_path.display().to_string()];

        // An absolute path either exists or it doesn't; no fallback searching.
        if !config_path.is_absolute() {
            // Relative to the working directory.
            let in_working_dir = working_dir.join(config_path);
            if in_working_dir.exists() {
                return Ok(in_working_dir);
            }
            tried_locations.push(in_working_dir.display().to_string());

            // The user config directory ($XDG_CONFIG_HOME/oober or ~/.config/oober).
            if let Some(config_dir) = user_config_dir {
                let user_config = config_dir.join(config_path);
                if user_config.exists() {
                    return Ok(user_config);
                }
                tried_locations.push(user_config.display().to_string());
            }
        }

        Err(format!(
            "Config file '{}' not found. Searched in:\n  - {}",
            config_path.display(),
            tried_locations.join("\n  - ")
        )
        .into())
    }

    /// The per-user config directory: `$XDG_CONFIG_HOME/oober`, falling back
    /// to `~/.config/oober`.
    fn user_config_dir() -> Option<PathBuf> {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("oober"));
            }
        }
        env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("oober"))
    }

    /// Loads a `ScanConfig` from a YAML file.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the YAML configuration file.
    pub fn load_scan_config(path: &Path) -> Result<ScanConfig> {
        let file = File::open(path).map_err(|e| {
            Error::Config(format!(
                "could not open patterns file '{}': {} (pass -e '<regex>' to scan without a file, or -p <file> to use a different one)",
                path.display(),
                e
            ))
        })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_find_config_uses_path_as_given() {
        let temp = TempDir::new().unwrap();
        let cfg = temp.path().join("direct.yaml");
        fs::write(&cfg, "patterns: []\n").unwrap();

        let found = ConfigLoader::find_config_in(&cfg, temp.path(), None).unwrap();
        assert_eq!(found, cfg);
    }

    #[test]
    fn test_find_config_falls_back_to_working_dir() {
        let temp = TempDir::new().unwrap();
        let work = temp.path().join("work");
        fs::create_dir(&work).unwrap();
        fs::write(work.join("wd.yaml"), "patterns: []\n").unwrap();

        let found = ConfigLoader::find_config_in(Path::new("wd.yaml"), &work, None).unwrap();
        assert_eq!(found, work.join("wd.yaml"));
    }

    #[test]
    fn test_find_config_falls_back_to_user_config_dir() {
        let temp = TempDir::new().unwrap();
        let work = temp.path().join("work");
        fs::create_dir(&work).unwrap();
        let user_dir = temp.path().join("xdg").join("oober");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("user.yaml"), "patterns: []\n").unwrap();

        let found =
            ConfigLoader::find_config_in(Path::new("user.yaml"), &work, Some(&user_dir)).unwrap();
        assert_eq!(found, user_dir.join("user.yaml"));
    }

    #[test]
    fn test_find_config_error_lists_exactly_the_searched_locations() {
        let temp = TempDir::new().unwrap();
        let work = temp.path().join("work");
        fs::create_dir(&work).unwrap();
        let user_dir = temp.path().join("xdg").join("oober");

        let err = ConfigLoader::find_config_in(Path::new("missing.yaml"), &work, Some(&user_dir))
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing.yaml"), "{msg}");
        assert!(
            msg.contains(work.join("missing.yaml").to_str().unwrap()),
            "{msg}"
        );
        assert!(
            msg.contains(user_dir.join("missing.yaml").to_str().unwrap()),
            "{msg}"
        );
        assert!(!msg.contains(".uber_scanner"), "{msg}");
    }

    #[test]
    fn test_find_config_does_not_search_absolute_paths_elsewhere() {
        let temp = TempDir::new().unwrap();
        let user_dir = temp.path().join("xdg").join("oober");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("abs.yaml"), "patterns: []\n").unwrap();

        // An absolute path that does not exist must not fall back to the
        // working dir or the user config dir, even if a same-named file exists there.
        let missing_abs = temp.path().join("abs.yaml");
        let err =
            ConfigLoader::find_config_in(&missing_abs, &user_dir, Some(&user_dir)).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_find_config_ignores_executable_directory() {
        // The pre-audit search chain picked up configs next to the binary
        // (and in its parent/grandparent, i.e. inside target/). Plant one
        // there and prove it is no longer found.
        let exe_dir = env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let name = format!("oober_exe_dir_cfg_{}.yaml", std::process::id());
        let planted = exe_dir.join(&name);
        fs::write(&planted, "patterns: []\n").unwrap();

        // The user config dir is injected so the lookup cannot probe the
        // real one.
        let temp = TempDir::new().unwrap();
        let user_dir = temp.path().join("xdg").join("oober");
        let result = ConfigLoader::find_config_in(Path::new(&name), temp.path(), Some(&user_dir));
        fs::remove_file(&planted).unwrap();

        let err = result.expect_err("a config next to the binary must not be picked up");
        assert!(err.to_string().contains("not found"));
    }
}
