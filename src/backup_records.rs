//! Records of the `.bak` files `replace` created.
//!
//! A `*.bak` file can be the user's own, so `undo` and `clean-backups` act
//! on a backup by default only when a record says oober made it. Each backup
//! has one small JSON file under `<cache dir>/oober/backups/`, named by the
//! SHA-256 of the backup's path: parallel workers never share a file.

use crate::errors::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tempfile::NamedTempFile;

/// What oober knows about one backup it created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BackupRecord {
    /// The backup file, in the form [`canonical_path`] returns.
    pub backup: PathBuf,
    /// The file the backup belongs to, in the same form.
    pub original: PathBuf,
    /// The size of the original as oober left it after its last write.
    pub size: u64,
    /// The modification time of the original after that write.
    pub mtime: SystemTime,
    /// When the backup was created.
    pub created: SystemTime,
}

/// The directory that holds the records.
///
/// It sits beside the incremental-scan cache, under the same base as
/// `StateManager`. Unit tests get a per-process temp dir instead, so they
/// never read or write the real cache.
fn records_dir() -> PathBuf {
    #[cfg(test)]
    {
        use std::sync::OnceLock;
        static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
        DIR.get_or_init(|| tempfile::TempDir::new().expect("temp dir must be creatable"))
            .path()
            .to_path_buf()
    }
    #[cfg(not(test))]
    {
        crate::state_manager::StateManager::base_cache_dir(std::env::var_os("XDG_CACHE_HOME"))
            .join("oober")
            .join("backups")
    }
}

/// The absolute form of `path` that records are keyed by and store: the
/// canonical parent directory plus the file name.
///
/// The last component is never resolved, so a symlink sitting at a backup
/// path cannot be mistaken for the backup recorded at its target, and the
/// path need not exist.
pub(crate) fn canonical_path(path: &Path) -> io::Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no file name", path.display()),
        )
    })?;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Ok(fs::canonicalize(parent)?.join(name))
}

/// The record file for a backup given in [`canonical_path`] form.
fn record_file(canonical_backup: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(canonical_backup.as_os_str().as_encoded_bytes());
    records_dir().join(format!("{:x}.json", hasher.finalize()))
}

/// Records that `backup` holds the pre-oober content of `original`, along
/// with the size and modification time `original` has right now.
///
/// Call it after oober has written `original`. `created` is the current time
/// for a new backup and the stored value when a record is refreshed.
pub(crate) fn write(backup: &Path, original: &Path, created: SystemTime) -> Result<()> {
    let metadata = fs::metadata(original)?;
    let record = BackupRecord {
        backup: canonical_path(backup)?,
        original: canonical_path(original)?,
        size: metadata.len(),
        mtime: metadata.modified()?,
        created,
    };

    let dir = records_dir();
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }

    // Atomic write, owner-only like the scan cache.
    let mut temp_file = NamedTempFile::new_in(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp_file
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temp_file.write_all(serde_json::to_string(&record)?.as_bytes())?;
    temp_file.persist(record_file(&record.backup))?;
    Ok(())
}

/// The record for `backup`, if oober has one. Never creates anything.
pub(crate) fn load(backup: &Path) -> Option<BackupRecord> {
    let contents = fs::read_to_string(record_file(&canonical_path(backup).ok()?)).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Deletes the record for `backup`; a missing record is not an error.
pub(crate) fn remove(backup: &Path) {
    if let Ok(canonical) = canonical_path(backup) {
        let _ = fs::remove_file(record_file(&canonical));
    }
}

/// Deletes the record of a backup that no longer exists. `canonical_backup`
/// is the path as its record stores it; it is not resolved again, because
/// the backup's directory may be gone too.
pub(crate) fn remove_stale(canonical_backup: &Path) {
    let _ = fs::remove_file(record_file(canonical_backup));
}

/// Every record whose backup lies under `canonical_dir`.
///
/// A missing record directory is an empty list, and entries that cannot be
/// read or parsed are skipped: another process may be writing or removing
/// them. Never creates anything.
pub(crate) fn list_under(canonical_dir: &Path) -> Vec<BackupRecord> {
    let Ok(entries) = fs::read_dir(records_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|contents| serde_json::from_str::<BackupRecord>(&contents).ok())
        .filter(|record| record.backup.starts_with(canonical_dir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_record_round_trip_list_and_remove() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let original = dir.join("a.txt");
        let backup = dir.join("a.txt.bak");
        fs::write(&original, "after").unwrap();
        fs::write(&backup, "before").unwrap();
        let canonical_dir = fs::canonicalize(dir).unwrap();

        assert!(load(&backup).is_none());
        assert!(list_under(&canonical_dir).is_empty());

        let created = SystemTime::now();
        write(&backup, &original, created).unwrap();

        let record = load(&backup).expect("a written record must load");
        assert_eq!(record.backup, canonical_dir.join("a.txt.bak"));
        assert_eq!(record.original, canonical_dir.join("a.txt"));
        assert_eq!(record.size, 5);
        assert_eq!(
            record.mtime,
            fs::metadata(&original).unwrap().modified().unwrap()
        );
        assert_eq!(record.created, created);

        // Listing is scoped to the directory asked for.
        let listed = list_under(&canonical_dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].backup, record.backup);
        assert!(list_under(&canonical_dir.join("elsewhere")).is_empty());

        remove(&backup);
        assert!(load(&backup).is_none());
        assert!(list_under(&canonical_dir).is_empty());
        // Removing a record that is already gone is fine.
        remove(&backup);
    }

    #[cfg(unix)]
    #[test]
    fn test_records_are_owner_only_and_keyed_without_following_symlinks() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();
        let original = dir.join("a.txt");
        let backup = dir.join("a.txt.bak");
        fs::write(&original, "after").unwrap();
        fs::write(&backup, "before").unwrap();
        write(&backup, &original, SystemTime::now()).unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&records_dir()), 0o700);
        let file = record_file(&canonical_path(&backup).unwrap());
        assert_eq!(mode(&file), 0o600);

        // A symlink to a recorded backup is not itself a recorded backup.
        let link = dir.join("link.bak");
        std::os::unix::fs::symlink(&backup, &link).unwrap();
        assert!(load(&link).is_none());
    }
}
