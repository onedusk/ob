use std::path::PathBuf;
use thiserror::Error;

/// The primary error type for all operations in the `oober` application.
///
/// This enum uses `thiserror` to neatly wrap various kinds of errors that can occur,
/// from I/O issues to configuration parsing problems.
#[derive(Error, Debug)]
pub enum Error {
    /// An error related to file system I/O.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An error that occurred during regex compilation.
    #[error("Pattern compilation failed: {0}")]
    Regex(#[from] regex::Error),

    /// An error that occurred while parsing a YAML configuration file.
    #[error("Config parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// A general configuration-related error.
    #[error("Config error: {0}")]
    Config(String),

    /// An error from the `ignore` crate, which is used for directory traversal.
    #[error("Walk error: {0}")]
    Walk(#[from] ignore::Error),

    /// An error that occurred while building the Rayon thread pool.
    #[error("Thread pool error: {0}")]
    ThreadPool(#[from] rayon::ThreadPoolBuildError),

    /// An error related to persisting a temporary file.
    #[error("Tempfile error: {0}")]
    TempFile(#[from] tempfile::PersistError),

    /// An error related to CSV serialization.
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    /// An error related to JSON serialization.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A rename whose computed target already exists and would be overwritten.
    #[error("target already exists, refusing to overwrite: {0}")]
    RenameCollision(PathBuf),

    /// A rename whose computed target escapes the source file's directory.
    #[error("target escapes the source directory: {0}")]
    RenameEscape(PathBuf),

    /// A rename run that finished with per-file errors.
    #[error("rename finished with {0} error(s)")]
    RenameFailed(usize),

    /// A replace run that finished with per-file errors.
    #[error("replace finished with {0} file(s) failed")]
    ReplaceFailed(usize),

    /// A file `replace` refuses to change: something it cannot treat as this
    /// file's backup already sits at the `.bak` path.
    #[error(
        "backup {0} already exists and oober cannot tell that it still matches this file, so the file was left unchanged; run `ob undo` to restore the backup or `ob clean-backups` to remove it (add --all-bak if oober did not create it), or pass --no-backup"
    )]
    BackupExists(PathBuf),

    /// A file that was modified, but whose backup could not be recorded.
    #[error(
        "the file was modified, but its backup {0} could not be recorded ({1}); `ob undo` restores it only with --all-bak"
    )]
    BackupRecord(PathBuf, String),

    /// An undo run that finished with per-file errors.
    #[error("undo finished with {0} file(s) failed")]
    UndoFailed(usize),
}

/// A convenient type alias for `Result<T, oober::errors::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Config(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::Config(s.to_string())
    }
}
