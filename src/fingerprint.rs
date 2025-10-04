use std::path::{Path, PathBuf};
use std::fs::{File, Metadata};
use std::io::{BufReader, Read};
use std::time::SystemTime;
use sha2::{Sha256, Digest};
use crate::errors::Result;
use crate::state_manager::FileState;

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
        let modified = metadata.modified()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let size = metadata.len();
        
        Ok(format!("{}-{}", modified, size))
    }
    
    /// Compares a file's current state to a cached `FileState` to see if it has changed.
    ///
    /// In quick mode, this only checks the modification time. In content hash mode,
    /// it compares the full content hash.
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
    use tempfile::NamedTempFile;
    use std::io::Write;
    
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
    
    #[test]
    fn test_quick_mode_performance() {
        use std::time::Instant;
        
        let file = NamedTempFile::new().unwrap();
        // Write 1MB of data
        let data = vec![0u8; 1_000_000];
        std::fs::write(file.path(), &data).unwrap();
        
        let quick = Fingerprinter::new(false);
        let full = Fingerprinter::new(true);
        
        let start = Instant::now();
        quick.fingerprint_file(file.path()).unwrap();
        let quick_time = start.elapsed();
        
        let start = Instant::now();
        full.fingerprint_file(file.path()).unwrap();
        let full_time = start.elapsed();
        
        // Quick mode should be significantly faster
        assert!(quick_time < full_time / 5);
    }
}