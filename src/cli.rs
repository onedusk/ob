use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A blazing-fast code scanner and transformer for monoliths.
///
/// `oober` is a command-line tool designed for large-scale codebase maintenance.
/// It can scan for patterns, perform complex replacements, and rename files,
/// all with a focus on speed and efficiency.
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "⚡ Ultra-fast pattern scanner and code transformer",
    long_about = "oober (ob) - A blazing-fast tool for scanning, replacing, and transforming code at scale.

Designed for large codebases with:
  • SIMD-powered regex matching
  • Parallel file processing
  • Automatic backup creation
  • Gitignore awareness
  • Smart empty line cleanup

QUICK EXAMPLES:
  ob scan .                           # Scan current directory with default patterns
  ob replace -d . --preset CleanDebug # Remove all debug code
  ob rename -d . -p 'test_(.*)' -r 'spec_$1'  # Rename test files
  ob undo -d .                        # Restore from backups

For detailed help on any command, use: ob <command> --help"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

/// Pre-defined sets of patterns for common replacement tasks.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum Preset {
    /// Remove copyright headers and license blocks.
    RemoveCopyright,
    /// Clean up debug code (e.g., `console.log`, `print` statements).
    CleanDebug,
    /// Remove `TODO` and `FIXME` comments.
    RemoveTodos,
    /// Clean up trailing whitespace from lines.
    TrimWhitespace,
    /// Remove empty, multi-line comment blocks.
    RemoveEmptyComments,
    /// Convert hard tabs to a specified number of spaces (default: 4).
    TabsToSpaces,
    /// Convert sequences of spaces to hard tabs.
    SpacesToTabs,
}

/// The set of available commands for the `oober` CLI.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scan files for patterns (security issues, TODOs, code smells, etc.)
    ///
    /// EXAMPLES:
    ///   ob scan .                              # Scan current dir with default patterns
    ///   ob scan -p security.yaml src/ lib/     # Scan for security issues
    ///   ob scan -x js,ts -o results.txt .      # Scan only JS/TS files
    ///   ob scan -f json . | jq '.matches[]'    # Output as JSON
    ///
    /// Pattern files use YAML format:
    ///   patterns:
    ///     - name: aws_key
    ///       pattern: 'AKIA[0-9A-Z]{16}'
    ///     - name: todo
    ///       pattern: 'TODO|FIXME|HACK'
    Scan {
        /// Path to the YAML file defining the scan patterns.
        #[arg(short, long, default_value = "patterns.yaml")]
        patterns: PathBuf,

        /// Path to the output file. If omitted, results are written to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// A comma-separated list of file extensions to include in the scan.
        #[arg(short = 'x', long = "ext", value_delimiter = ',')]
        extensions: Vec<String>,

        /// The number of parallel worker threads to use. Defaults to the number of logical CPU cores.
        #[arg(short = 'w', long = "workers", env = "UBER_SCANNER_WORKERS")]
        workers: Option<usize>,

        /// Enable incremental scanning. Only changed files will be re-scanned.
        #[arg(long = "incremental", short = 'i')]
        incremental: bool,

        /// Force a full re-scan of all files, ignoring any cached results.
        #[arg(long = "force-full")]
        force_full: bool,

        /// Clear the cache before starting the scan.
        #[arg(long = "clear-cache")]
        clear_cache: bool,

        /// Use file content hashes for change detection instead of modification timestamps.
        #[arg(long = "content-hash")]
        content_hash: bool,

        /// The output format for the scan results (e.g., `text`, `json`, `csv`, `sarif`, `html`).
        #[arg(short = 'f', long = "format", default_value = "text")]
        format: String,

        /// Include a summary of scan statistics in the output.
        #[arg(long = "summary")]
        include_summary: bool,

        /// The input files or directories to scan.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
    },

    /// Replace patterns in files (with automatic backups)
    ///
    /// EXAMPLES:
    ///   ob replace -d . --preset CleanDebug        # Remove all debug code
    ///   ob replace -d . -p 'TODO' -r 'TODO'        # Simple replacement
    ///   ob replace -d . -c config.yaml --dry-run   # Preview changes
    ///   ob replace -d src/ -x py --preset RemoveTodos
    ///
    /// Config file format (config.yaml):
    ///   patterns: ['console\\.log', 'debugger;']
    ///   replacements: ['// removed', null]  # null = delete line
    ///   blocks:
    ///     - start: '/* DEBUG START */'
    ///       end: '/* DEBUG END */'
    ///
    /// Available presets: RemoveCopyright, CleanDebug, RemoveTodos, TrimWhitespace
    Replace {
        /// The name of a built-in preset to use for replacement.
        #[arg(long, value_enum)]
        preset: Option<Preset>,

        /// Path to a YAML configuration file for replacement rules.
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// A single regex pattern to search for.
        #[arg(short, long)]
        pattern: Option<String>,

        /// The string to replace the matched pattern with.
        #[arg(short, long)]
        replacement: Option<String>,

        /// The directory to process.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// A comma-separated list of file extensions to include.
        #[arg(short = 'x', long = "ext", value_delimiter = ',')]
        extensions: Vec<String>,

        /// A comma-separated list of directories to exclude.
        #[arg(short = 'e', long = "exclude", value_delimiter = ',')]
        exclude: Vec<String>,

        /// Disable the creation of backup files (`.bak`).
        #[arg(long)]
        no_backup: bool,

        /// Preview the changes without actually modifying any files.
        #[arg(long)]
        dry_run: bool,

        /// Print each modified file (useful for audits; slower on large runs).
        #[arg(short, long)]
        verbose: bool,

        /// The number of parallel worker threads to use.
        #[arg(short, long)]
        workers: Option<usize>,
    },

    /// Restore files from backups (undo replacements)
    ///
    /// EXAMPLES:
    ///   ob undo -d .                    # Restore all files in current dir
    ///   ob undo -d src/ --keep-backups  # Restore but keep .bak files
    Undo {
        /// The directory where the `replace` operation was run.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// Keep the backup files after restoring the original files.
        #[arg(long)]
        keep_backups: bool,
    },

    /// Remove backup files without restoring
    ///
    /// EXAMPLES:
    ///   ob clean-backups -d . --dry-run  # Preview what would be deleted
    ///   ob clean-backups -d .            # Delete all .bak files
    CleanBackups {
        /// The directory to clean of backup files.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// Preview which backup files would be removed without deleting them.
        #[arg(long)]
        dry_run: bool,
    },

    /// Batch rename files using regex patterns
    ///
    /// EXAMPLES:
    ///   ob rename -d . -p 'test_(.*)' -r 'spec_$1'      # test_*.js -> spec_*.js
    ///   ob rename -d . -p '\\.tsx$' -r '.jsx' --dry-run  # Preview .tsx -> .jsx
    ///   ob rename -d . -p '(\\d+)_(.*)' -r '$2_$1'      # Reorder name parts
    ///
    /// Supports regex capture groups: $1, $2, etc.
    Rename {
        /// The directory containing files to rename.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// The regex pattern to match against filenames.
        #[arg(short, long, required = true)]
        pattern: String,

        /// The replacement string. Can include capture groups from the pattern (e.g., `$1`).
        #[arg(short, long, required = true)]
        replacement: String,

        /// Preview the renames without actually renaming any files.
        #[arg(long)]
        dry_run: bool,

        /// Print each renamed file (slower on large runs).
        #[arg(short, long)]
        verbose: bool,

        /// The number of parallel worker threads to use.
        #[arg(short, long)]
        workers: Option<usize>,
    },
}

/// Parses command-line arguments and returns the populated `Args` struct.
pub fn parse_args() -> Args {
    Args::parse()
}
