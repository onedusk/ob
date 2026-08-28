use crate::errors::Result;
use crate::state_manager::FileState;
use sha2::{Digest, Sha256};
use std::fs::{File, Metadata};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A utility for creating "fingerprints" of files to detect changes.
///
/// `Fingerprinter` can operate in two modes:
/// - **Content Hash Mode**: Computes a SHA-256 hash of the file's contents. This is
///   accurate but slower as it requires reading the entire file.
/// - **Quick Mode**: Uses file metadata (modification time and size) to create a
///   "hash". This is much faster but can miss changes that don't affect metadata.
pub struct Fingerprinter {
    use_content_hash: bool,
    quick_mode: bool,
}

impl Fingerprinter {
    /// Creates a new `Fingerprinter`.
    ///
    /// # Arguments
    ///
    /// * `use_content_hash` - If `true`, the fingerprinter will use content hashing.
    ///   Otherwise, it will use the quicker metadata-based approach.
    pub fn new(use_content_hash: bool) -> Self {
        Self {
            use_content_hash,
            quick_mode: !use_content_hash,
        }
    }

    /// Generates a `FileFingerprint` for a given file path.
    ///
    /// Depending on the mode, this will either compute a full content hash or
    /// a quick metadata-based hash.
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

    /// Computes a SHA-256 hash of a file's contents.
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

    /// Creates a quick "hash" from file metadata.
    ///
    /// The hash is a combination of the file's modification timestamp (in seconds
    /// since the UNIX epoch) and its size in bytes.
    fn compute_quick_hash(&self, metadata: &Metadata) -> Result<String> {
        // Quick hash based on metadata only
        let modified = metadata
            .modified()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let size = metadata.len();

        Ok(format!("{}-{}", modified, size))
    }

    /// Compares a file's current state to a cached `FileState` to see if it has changed.
    ///
    /// The cheap metadata comparison runs first: a size mismatch reports a change
    /// without reading the file. Only when sizes match does quick mode compare the
    /// modification time and content hash mode compute and compare the full
    /// content hash.
    pub fn has_file_changed(&self, path: &Path, cached_state: &FileState) -> Result<bool> {
        Ok(self.check_file(path, cached_state)?.0)
    }

    /// Like [`Self::has_file_changed`], but also returns the fresh
    /// `FileFingerprint` when the comparison had to compute one, so callers can
    /// reuse it instead of reading the file again.
    ///
    /// The fingerprint is `None` when the file is unchanged (the cached state is
    /// still valid) and when a size mismatch short-circuited the comparison
    /// before any hash was computed.
    pub fn check_file(
        &self,
        path: &Path,
        cached_state: &FileState,
    ) -> Result<(bool, Option<FileFingerprint>)> {
        let metadata = path.metadata()?;

        // Quick check: a size mismatch always means a change, so the
        // (potentially expensive) content hash is never needed.
        if metadata.len() != cached_state.size {
            return Ok((true, None));
        }

        let modified = metadata.modified()?;
        let hash = if self.quick_mode {
            // Just check modification time
            if modified == cached_state.modified {
                return Ok((false, None));
            }
            self.compute_quick_hash(&metadata)?
        } else {
            // Full content hash comparison
            let hash = self.compute_content_hash(path)?;
            if hash == cached_state.hash {
                return Ok((false, None));
            }
            hash
        };

        Ok((
            true,
            Some(FileFingerprint {
                path: path.to_path_buf(),
                modified,
                size: metadata.len(),
                hash,
            }),
        ))
    }
}

/// Contains the metadata and hash that uniquely identify the state of a file.
pub struct FileFingerprint {
    /// The path to the file.
    pub path: PathBuf,
    /// The last modification timestamp.
    pub modified: SystemTime,
    /// The size of the file in bytes.
    pub size: u64,
    /// The computed hash (either content-based or metadata-based).
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_content_hash_changes() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"Hello, World!").unwrap();
        file.flush().unwrap();

        let fingerprinter = Fingerprinter::new(true);
        let fp1 = fingerprinter.fingerprint_file(file.path()).unwrap();

        // Modify file
        file.write_all(b" Modified").unwrap();
        file.flush().unwrap();

        let fp2 = fingerprinter.fingerprint_file(file.path()).unwrap();

        assert_ne!(fp1.hash, fp2.hash);
    }

    fn cached_state_from(fp: &FileFingerprint) -> FileState {
        FileState {
            path: fp.path.clone(),
            modified: fp.modified,
            size: fp.size,
            hash: fp.hash.clone(),
            last_scanned: SystemTime::now(),
        }
    }

    #[test]
    fn test_content_hash_mode_detects_same_size_edit() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"aaaa").unwrap();

        let fingerprinter = Fingerprinter::new(true);
        let cached = cached_state_from(&fingerprinter.fingerprint_file(file.path()).unwrap());

        // Same size, different content: only the content hash can see this.
        std::fs::write(file.path(), b"bbbb").unwrap();
        assert!(
            fingerprinter
                .has_file_changed(file.path(), &cached)
                .unwrap()
        );

        // Identical content is reported unchanged.
        std::fs::write(file.path(), b"aaaa").unwrap();
        assert!(
            !fingerprinter
                .has_file_changed(file.path(), &cached)
                .unwrap()
        );
    }

    #[test]
    fn test_check_file_returns_fingerprint_only_when_computed() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"aaaa").unwrap();

        let fingerprinter = Fingerprinter::new(true);
        let cached = cached_state_from(&fingerprinter.fingerprint_file(file.path()).unwrap());

        // Unchanged: the cached state is still valid, no fingerprint returned.
        let (changed, fingerprint) = fingerprinter.check_file(file.path(), &cached).unwrap();
        assert!(!changed);
        assert!(fingerprint.is_none());

        // Same-size edit: the hash had to be computed, so it is handed back
        // for reuse and matches the new content.
        std::fs::write(file.path(), b"bbbb").unwrap();
        let (changed, fingerprint) = fingerprinter.check_file(file.path(), &cached).unwrap();
        assert!(changed);
        let fingerprint = fingerprint.expect("computed fingerprint must be returned");
        assert_eq!(
            fingerprint.hash,
            fingerprinter.fingerprint_file(file.path()).unwrap().hash
        );

        // Size mismatch: short-circuited before hashing, no fingerprint.
        std::fs::write(file.path(), b"longer content").unwrap();
        let (changed, fingerprint) = fingerprinter.check_file(file.path(), &cached).unwrap();
        assert!(changed);
        assert!(fingerprint.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_size_mismatch_short_circuits_content_hash() {
        use std::os::unix::fs::PermissionsExt;

        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"original").unwrap();

        let fingerprinter = Fingerprinter::new(true);
        let mut cached = cached_state_from(&fingerprinter.fingerprint_file(file.path()).unwrap());
        cached.size += 1; // mismatched size: metadata alone must decide

        // Make the file unreadable: hashing it would fail, so a successful
        // has_file_changed proves the size check short-circuited the hash.
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o000)).unwrap();
        let changed = fingerprinter.has_file_changed(file.path(), &cached);
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(changed.unwrap());
    }

    #[test]
    fn test_quick_mode_hash_is_metadata_based() {
        let file = NamedTempFile::new().unwrap();
        let data = vec![0u8; 1_000_000];
        std::fs::write(file.path(), &data).unwrap();

        let quick = Fingerprinter::new(false);
        let full = Fingerprinter::new(true);

        let quick_fp = quick.fingerprint_file(file.path()).unwrap();
        let full_fp = full.fingerprint_file(file.path()).unwrap();

        // Quick mode produces an "mtime-size" hash, not a content digest.
        let (mtime, size) = quick_fp
            .hash
            .split_once('-')
            .expect("quick hash should have the form <mtime>-<size>");
        assert!(mtime.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(size, "1000000");

        // Content-hash mode produces a SHA-256 hex digest; the two must differ.
        assert_eq!(full_fp.hash.len(), 64);
        assert!(full_fp.hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(quick_fp.hash, full_fp.hash);
    }
}
