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

    /// An error that occurred during the processing of a single file.
    #[error("File processing failed for {path}: {source}")]
    Processing {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

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
    
    /// An error that occurred converting a byte slice to a UTF-8 string.
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    
    /// An error from the `walkdir` crate.
    #[error("Walkdir error: {0}")]
    WalkDir(#[from] walkdir::Error),
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
