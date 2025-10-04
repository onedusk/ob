//! `oober` is a library for high-performance file scanning and replacement operations.
//!
//! It provides the core logic for the `oober` command-line tool but can also be used
//! as a standalone library. The main components are:
//!
//! - `Scanner`: For finding patterns in files using regex, with support for incremental
//!   scans and various output formats.
//! - `Replacer`: For performing find-and-replace operations in files, with support
//!   for backups and dry runs.
//! - `file_renamer`: For batch renaming of files.
//! - `config`: For loading scan and replacement configurations from YAML files.
//! - `state_manager`: For caching scan results to speed up subsequent runs.
//!
//! The library is designed to be fast, using parallel processing with Rayon and
//! efficient directory traversal with the `ignore` crate.

pub mod cli;
pub mod config;
pub mod errors;
pub mod file_renamer;
pub mod fingerprint;
pub mod output_formatter;
pub mod patterns;
pub mod replacer;
pub mod scanner;
pub mod state_manager;

// Re-export main types for easier access by library users.
pub use errors::{Error, Result};
pub use output_formatter::{OutputFormat, OutputFormatter};
pub use replacer::Replacer;
pub use scanner::Scanner;
pub use state_manager::StateManager;
