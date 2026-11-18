use crate::errors::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Represents the overall state of a scan, including metadata and file details.
/// This struct is serialized to and from a JSON file to cache scan results.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScanState {
    /// The version of the tool that created the state, used for compatibility checks.
    pub version: String,
    /// The timestamp of the last scan.
    pub last_scan: SystemTime,
    /// A map of file paths to their last known state.
    pub files: HashMap<PathBuf, FileState>,
    /// A hash of the patterns used in the last scan, to detect changes.
    pub patterns_hash: String,
    /// Cached scan results for each file.
    pub scan_results: HashMap<PathBuf, Vec<CachedMatch>>,
}

/// Holds metadata about a single file to determine if it needs to be re-scanned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    /// The path to the file.
    pub path: PathBuf,
    /// The last modification timestamp of the file.
    pub modified: SystemTime,
    /// The size of the file in bytes.
    pub size: u64,
    /// A hash of the file's contents.
    pub hash: String,
    /// The timestamp when this file was last scanned.
    pub last_scanned: SystemTime,
}

/// A cached result of a pattern match within a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMatch {
    /// The name of the pattern that matched.
    pub pattern_name: String,
    /// The line number where the match occurred.
    pub line_number: usize,
    /// The content of the line that matched. Empty when the cache was written
    /// with `--no-cache-content`; consumers reconstruct the text by re-reading
    /// the recorded line from the (fingerprint-unchanged) file.
    #[serde(default)]
    pub line_content: String,
}

/// Manages the persistence of scan state for a project.
///
/// `StateManager` is responsible for loading and saving the `ScanState` to a cache
/// directory. It computes a unique project ID based on the project's root path
/// to avoid state collisions between different projects.
pub struct StateManager {
    state_dir: PathBuf,
    project_id: String,
}

impl StateManager {
    /// Creates a new `StateManager` for a given project root.
    ///
    /// This function determines the appropriate cache directory and computes a
    /// project-specific ID. It also ensures the cache directory exists.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the project.
    pub fn new(project_root: &Path) -> Result<Self> {
        Self::with_state_dir(Self::get_state_dir()?, project_root)
    }

    /// Creates a `StateManager` that stores state in the given directory,
    /// creating it if needed.
    ///
    /// Cache files store the matched line contents of previous scans in
    /// plaintext (potentially secrets), so on Unix the directory is kept
    /// owner-only (0700) — including a pre-existing directory created by an
    /// older version with looser permissions.
    fn with_state_dir(state_dir: PathBuf, project_root: &Path) -> Result<Self> {
        let project_id = Self::compute_project_id(project_root)?;

        // Ensure state directory exists
        fs::create_dir_all(&state_dir)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700))?;
        }

        Ok(Self {
            state_dir,
            project_id,
        })
    }

    /// Determines the directory for storing state files.
    ///
    /// It respects the `XDG_CACHE_HOME` environment variable if set and
    /// non-empty, otherwise it falls back to the default user cache directory.
    fn get_state_dir() -> Result<PathBuf> {
        Ok(Self::base_cache_dir(std::env::var_os("XDG_CACHE_HOME")).join("oober"))
    }

    /// The base cache directory for a given `XDG_CACHE_HOME` value.
    ///
    /// A set-but-empty `XDG_CACHE_HOME` is ignored exactly like an unset one
    /// (matching the `XDG_CONFIG_HOME` handling in `config.rs`): treating `""`
    /// as valid resolved to the relative path `./oober` — a plaintext cache of
    /// matched lines dropped inside the scanned project, where it can get
    /// committed.
    fn base_cache_dir(xdg_cache_home: Option<std::ffi::OsString>) -> PathBuf {
        if let Some(xdg) = xdg_cache_home {
            if !xdg.is_empty() {
                return PathBuf::from(xdg);
            }
        }
        dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".cache"))
    }

    /// Computes a unique ID for the project based on its canonical path.
    ///
    /// This ensures that different projects have separate cache files.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the project.
    fn compute_project_id(project_root: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let canonical = project_root.canonicalize()?;
        let path_str = canonical.to_string_lossy();

        let mut hasher = Sha256::new();
        hasher.update(path_str.as_bytes());
        let result = hasher.finalize();

        Ok(format!("{:x}", result))
    }

    /// Loads the `ScanState` from the cache file for the current project.
    ///
    /// If the cache file does not exist, or if the version in the cache file
    /// does not match the current tool version, it returns `Ok(None)`.
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

    /// Saves the `ScanState` to the cache file for the current project.
    ///
    /// The save operation is performed atomically by writing to a temporary file
    /// first and then renaming it.
    ///
    /// # Optimization Note
    ///
    /// This function uses `serde_json::to_string_pretty` for human-readable JSON.
    /// For performance-critical applications, switching to `serde_json::to_string`
    /// would be faster and result in smaller file sizes.
    ///
    /// # Arguments
    ///
    /// * `state` - The `ScanState` to save.
    pub fn save_state(&self, state: &ScanState) -> Result<()> {
        let state_file = self.state_file_path();
        let contents = serde_json::to_string_pretty(state)?;

        // Atomic write using tempfile
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new_in(&self.state_dir)?;
        // The cache holds matched line contents in plaintext; make the
        // persisted file owner-only before it lands at its final path.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp_file
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        temp_file.write_all(contents.as_bytes())?;
        temp_file.persist(state_file)?;

        Ok(())
    }

    /// Constructs the full path to the state file for the current project.
    fn state_file_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}.json", self.project_id))
    }

    /// Deletes the cache file for the current project.
    pub fn clear_cache(&self) -> Result<()> {
        let state_file = self.state_file_path();
        if state_file.exists() {
            fs::remove_file(state_file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        // `StateManager::new` would write into the real user cache dir.
        let manager =
            StateManager::with_state_dir(temp_dir.path().join("cache"), temp_dir.path()).unwrap();

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
            },
        );

        // Save and load
        manager.save_state(&state).unwrap();
        let loaded = manager.load_state().unwrap().unwrap();

        assert_eq!(loaded.patterns_hash, state.patterns_hash);
        assert_eq!(loaded.files.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_cache_dir_and_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let state_dir = temp_dir.path().join("cache").join("oober");
        // Pre-create with permissive mode: an existing dir from an older
        // version must be tightened, not just freshly created dirs.
        fs::create_dir_all(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o755)).unwrap();

        let manager = StateManager::with_state_dir(state_dir.clone(), temp_dir.path()).unwrap();

        let state = ScanState {
            version: env!("CARGO_PKG_VERSION").to_string(),
            last_scan: SystemTime::now(),
            files: HashMap::new(),
            patterns_hash: "perm_test".to_string(),
            scan_results: HashMap::new(),
        };
        manager.save_state(&state).unwrap();

        let dir_mode = fs::metadata(&state_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "cache dir must be owner-only");

        let file_mode = fs::metadata(manager.state_file_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600, "cache file must be owner-only");
    }

    #[test]
    fn test_base_cache_dir_ignores_empty_xdg_cache_home() {
        use std::ffi::OsString;

        // A set XDG_CACHE_HOME wins.
        assert_eq!(
            StateManager::base_cache_dir(Some(OsString::from("/custom/cache"))),
            PathBuf::from("/custom/cache")
        );

        // A set-but-empty XDG_CACHE_HOME must fall back like an unset one.
        // Treating "" as valid yielded the relative "./oober" — a plaintext
        // cache dropped inside the scanned project.
        let empty = StateManager::base_cache_dir(Some(OsString::new()));
        assert!(
            !empty.as_os_str().is_empty(),
            "empty XDG_CACHE_HOME must not yield a relative cache dir"
        );
        assert_eq!(empty, StateManager::base_cache_dir(None));
    }

    #[test]
    fn test_cached_match_line_content_defaults_to_empty() {
        // A cache entry without line_content (the field is only meaningful
        // when matched text is persisted) must keep parsing: the field
        // defaults to empty instead of failing deserialization.
        let m: CachedMatch =
            serde_json::from_str(r#"{"pattern_name":"p","line_number":3}"#).unwrap();
        assert_eq!(m.pattern_name, "p");
        assert_eq!(m.line_number, 3);
        assert!(m.line_content.is_empty());
    }

    #[test]
    fn test_version_invalidation() {
        let temp_dir = TempDir::new().unwrap();
        // `StateManager::new` would write into the real user cache dir.
        let manager =
            StateManager::with_state_dir(temp_dir.path().join("cache"), temp_dir.path()).unwrap();

        let state = ScanState {
            version: "0.0.1".to_string(), // Old version
            last_scan: SystemTime::now(),
            files: HashMap::new(),
            patterns_hash: "test".to_string(),
            scan_results: HashMap::new(),
        };

        manager.save_state(&state).unwrap();
        let loaded = manager.load_state().unwrap();

        // Should invalidate due to version mismatch
        assert!(loaded.is_none());
    }
}
