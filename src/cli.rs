use crate::errors::Result;
use crate::output_formatter::OutputFormat;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use std::io;
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
  ob scan .                           # Scan current dir using ./patterns.yaml
  ob scan -e 'TODO|FIXME' .           # Scan with an inline regex (no YAML file)
  ob replace . --preset clean-debug   # Remove debug code
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
    /// Comment out whole-line debug statements (`console.log`, `debugger`, `print(...)  # DEBUG`) and remove DEBUG START/END blocks.
    CleanDebug,
    /// Remove `TODO`, `FIXME`, `HACK` and `XXX` comments (`//` and `#` style), keeping the code before them.
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
    ///   ob scan .                              # Scan current dir using ./patterns.yaml
    ///   ob scan -e 'TODO|FIXME' .              # Inline regex, no YAML file needed
    ///   ob scan -p security.yaml src/ lib/     # Scan for security issues
    ///   ob scan -x js,ts -o results.txt .      # Scan only JS/TS files
    ///   ob scan -f json . | jq '.matches[]'    # Output as JSON
    ///   ob scan --hidden --no-ignore .         # Include hidden + gitignored files
    ///
    /// DEFAULT EXCLUSIONS: hidden files and directories (dotfiles such as .env,
    /// .github/) and anything matched by .gitignore/.ignore files are NOT
    /// scanned. For secret scanning, add --hidden and --no-ignore so likely
    /// secret locations are not silently skipped.
    ///
    /// Pattern files use YAML format:
    ///   patterns:
    ///     - name: aws_key
    ///       pattern: 'AKIA[0-9A-Z]{16}'
    ///     - name: todo
    ///       pattern: 'TODO|FIXME|HACK'
    ///   exclusions: [node_modules, vendor]   # optional
    ///
    /// `exclusions` lists names to skip, matched below each input directory;
    /// a file named as an input is never excluded. It has no effect with -e,
    /// which loads no patterns file. An unknown key in the file is an error.
    Scan {
        /// Path to the YAML file defining the scan patterns.
        #[arg(short, long, default_value = "patterns.yaml")]
        patterns: PathBuf,

        /// An inline regex to scan for instead of a patterns file. Repeatable (-e A -e B).
        /// When provided, the patterns file is ignored.
        #[arg(short = 'e', long = "regex")]
        regex: Vec<String>,

        /// Path to the output file. If omitted, results are written to standard output.
        /// The output file is never scanned itself, even inside a scanned directory.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// A comma-separated list of file extensions to include in the scan.
        #[arg(short = 'x', long = "ext", value_delimiter = ',')]
        extensions: Vec<String>,

        /// The number of parallel worker threads to use. Defaults to the number of logical CPU cores.
        #[arg(short = 'w', long = "workers", env = "OOBER_WORKERS")]
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

        /// Do not store matched line contents in the incremental scan cache;
        /// they are re-read from the unchanged files on cache hits. Requires --incremental.
        #[arg(long = "no-cache-content")]
        no_cache_content: bool,

        /// The output format for the scan results.
        #[arg(
            short = 'f',
            long = "format",
            value_enum,
            ignore_case = true,
            default_value = "text"
        )]
        format: OutputFormat,

        /// Include a summary of scan statistics in the output.
        #[arg(long = "summary")]
        include_summary: bool,

        /// Include hidden files and directories (skipped by default, e.g. `.env`).
        #[arg(long = "hidden")]
        hidden: bool,

        /// Do not respect .gitignore/.ignore files (respected by default).
        #[arg(long = "no-ignore")]
        no_ignore: bool,

        /// The input files or directories to scan.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
    },

    /// Replace patterns in files (with automatic backups)
    ///
    /// EXAMPLES:
    ///   ob replace . --preset clean-debug          # Remove debug code
    ///   ob replace . -p 'TODO' -r 'TODO'           # Simple replacement
    ///   ob replace src/ lib/ -c config.yaml --dry-run  # Preview changes
    ///   ob replace main.py --preset remove-todos   # Process a single file
    ///   ob replace --list-presets                  # List available presets
    ///
    /// Config file format (config.yaml):
    ///   patterns: ['console\\.log', 'debugger;']
    ///   replacements: ['// removed', null]  # null = delete the line AFTER each match (the matching line is kept)
    ///   blocks:
    ///     - start: '/* DEBUG START */'
    ///       end: '/* DEBUG END */'
    /// An unknown key in the config file is an error.
    ///
    /// Patterns are matched against the whole file content, not line-by-line:
    /// use (?m) for per-line ^/$ anchors (e.g. '(?m)[ \t]+$').
    ///
    /// Replacements can reference capture groups: $1 or ${1}, and ${name} for
    /// named groups. Use the ${1} form when a letter, digit or underscore
    /// follows the reference: '$1_x' refers to a group named '1_x'. Write $$
    /// for a literal dollar sign. A reference to a group the pattern does not
    /// define is an error.
    ///
    /// Symbolic links to files are never processed: each one is reported on
    /// stderr and left as a link. The file it points to is edited only when
    /// that file is itself among the inputs. A directory given as an input
    /// through a symlink is still walked.
    ///
    /// Available presets: remove-copyright, clean-debug, remove-todos, trim-whitespace,
    /// remove-empty-comments, tabs-to-spaces, spaces-to-tabs
    Replace {
        /// The name of a built-in preset to use for replacement.
        /// Cannot be combined with -c/--config or -p/--pattern.
        #[arg(long, value_enum, conflicts_with_all = ["config", "pattern"])]
        preset: Option<Preset>,

        /// List the available presets with their descriptions, then exit.
        #[arg(long)]
        list_presets: bool,

        /// Path to a YAML configuration file for replacement rules.
        /// The file itself is never processed, even inside an input directory.
        /// Cannot be combined with --preset or -p/--pattern.
        #[arg(short, long, conflicts_with = "pattern")]
        config: Option<PathBuf>,

        /// A single regex pattern to search for. Requires -r/--replacement.
        /// Cannot be combined with --preset or -c/--config.
        #[arg(short, long, requires = "replacement")]
        pattern: Option<String>,

        /// The string to replace the matched pattern with. Requires -p/--pattern.
        /// Can include capture groups (e.g., `$1` or `${1}`); `$$` is a literal dollar sign.
        #[arg(short, long, requires = "pattern")]
        replacement: Option<String>,

        /// The directory to process (legacy alias for a positional input).
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// A comma-separated list of file extensions to include.
        #[arg(short = 'x', long = "ext", value_delimiter = ',')]
        extensions: Vec<String>,

        /// A comma-separated list of directory names to exclude, matched
        /// below each input directory (not against the input's own path).
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

        /// The input files or directories to process. At least one input
        /// (positional or -d/--dir) is required unless --list-presets is given.
        inputs: Vec<PathBuf>,
    },

    /// Restore files from backups (undo replacements)
    ///
    /// Only the .bak files that `replace` recorded creating are restored;
    /// any other .bak file in the tree is left alone and counted.
    ///
    /// EXAMPLES:
    ///   ob undo -d .                    # Restore files from oober's backups
    ///   ob undo -d . --dry-run          # Preview what would be restored
    ///   ob undo -d src/ --keep-backups  # Restore but keep .bak files
    ///   ob undo -d . --all-bak          # Also restore .bak files oober did not record
    Undo {
        /// The directory where the `replace` operation was run.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// Keep the backup files after restoring the original files.
        #[arg(long)]
        keep_backups: bool,

        /// Also restore .bak files oober has no record of creating (backups
        /// made by 0.1.1, or made before the cache directory was wiped).
        #[arg(long)]
        all_bak: bool,

        /// Preview which files would be restored without changing anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove backup files without restoring
    ///
    /// Only the .bak files that `replace` recorded creating are removed;
    /// any other .bak file in the tree is left alone and counted.
    ///
    /// EXAMPLES:
    ///   ob clean-backups -d . --dry-run  # Preview what would be deleted
    ///   ob clean-backups -d .            # Delete oober's .bak files
    ///   ob clean-backups -d . --all-bak  # Also delete .bak files oober did not record
    CleanBackups {
        /// The directory to clean of backup files.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// Preview which backup files would be removed without deleting them.
        #[arg(long)]
        dry_run: bool,

        /// Also remove .bak files oober has no record of creating (backups
        /// made by 0.1.1, or made before the cache directory was wiped).
        #[arg(long)]
        all_bak: bool,
    },

    /// Batch rename files using regex patterns
    ///
    /// EXAMPLES:
    ///   ob rename -d . -p 'test_(.*)' -r 'spec_$1'      # test_*.js -> spec_*.js
    ///   ob rename -d . -p '\\.tsx$' -r '.jsx' --dry-run  # Preview .tsx -> .jsx
    ///   ob rename -d . -p '^(\d+)_(.+?)(\.[^.]+)?$' -r '${2}_${1}${3}'  # 12_intro.md -> intro_12.md
    ///
    /// The replacement can reference capture groups: $1 or ${1}, and ${name}
    /// for named groups. Use the ${1} form when a letter, digit or underscore
    /// follows the reference: '$1_x' refers to a group named '1_x'. Write $$
    /// for a literal dollar sign. A reference to a group the pattern does not
    /// define is an error.
    Rename {
        /// The directory containing files to rename.
        #[arg(short, long, required = true)]
        dir: PathBuf,

        /// The regex pattern to match against filenames.
        #[arg(short, long, required = true)]
        pattern: String,

        /// The replacement string. Can include capture groups from the pattern
        /// (e.g., `$1` or `${1}`); `$$` is a literal dollar sign.
        #[arg(short, long, required = true)]
        replacement: String,

        /// Preview the renames without actually renaming any files.
        #[arg(long)]
        dry_run: bool,

        /// Print each renamed file (slower on large runs).
        #[arg(short, long)]
        verbose: bool,

        /// Accepted for compatibility; renames always execute serially so collision checks cannot race.
        #[arg(short, long)]
        workers: Option<usize>,
    },

    /// Generate a shell completion script on stdout
    ///
    /// The script is written to standard output; redirect it into your
    /// shell's completion directory:
    ///
    ///   ob completions zsh > ~/.zfunc/_oober    # with ~/.zfunc in fpath
    ///   ob completions bash > ~/.local/share/bash-completion/completions/oober
    ///   ob completions fish > ~/.config/fish/completions/oober.fish
    Completions {
        /// The shell to generate a completion script for.
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Print the roff man page on stdout
    ///
    /// EXAMPLES:
    ///   ob manpage > oober.1 && man ./oober.1       # Preview without installing
    ///   ob manpage > /usr/local/share/man/man1/oober.1
    Manpage,
}

/// Parses command-line arguments and returns the populated `Args` struct.
pub fn parse_args() -> Args {
    Args::parse()
}

/// Returns the listing printed by `replace --list-presets`: one line per
/// preset with its kebab-case name and the one-line description taken from
/// the `Preset` enum's doc comments (via clap's `ValueEnum` metadata, so the
/// listing can never drift from `--help`).
pub fn preset_listing() -> String {
    use clap::ValueEnum;

    let mut out = String::new();
    for preset in Preset::value_variants() {
        // `to_possible_value` is only `None` for variants skipped with
        // `#[value(skip)]`, of which there are none.
        if let Some(value) = preset.to_possible_value() {
            let help = value.get_help().map(|h| h.to_string()).unwrap_or_default();
            out.push_str(&format!("{:<22} {}\n", value.get_name(), help));
        }
    }
    out
}

/// Writes the completion script for `shell` to standard output.
///
/// The script is generated for the installed binary name (`oober`); shells
/// resolve the `ob` alias to that name when completing, so one script covers
/// both spellings. Nothing is written to the filesystem.
pub fn run_completions(shell: Shell) {
    let mut cmd = Args::command();
    clap_complete::generate(shell, &mut cmd, "oober", &mut io::stdout());
}

/// Writes the top-level roff man page to standard output.
///
/// Only the top-level page is generated (subcommands appear in its
/// SUBCOMMANDS section). Nothing is written to the filesystem.
pub fn run_manpage() -> Result<()> {
    clap_mangen::Man::new(Args::command()).render(&mut io::stdout())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_format_rejects_unknown_values() {
        // A typo must fail at parse time, not silently fall back to text.
        assert!(Args::try_parse_from(["ob", "scan", "-f", "jsonn", "."]).is_err());
    }

    #[test]
    fn test_scan_format_accepts_known_values() {
        for fmt in ["text", "json", "csv", "sarif", "html"] {
            assert!(
                Args::try_parse_from(["ob", "scan", "-f", fmt, "."]).is_ok(),
                "format {fmt} must parse"
            );
        }
    }

    #[test]
    fn test_scan_format_is_case_insensitive() {
        // "-f JSON" worked before --format became a clap value_enum; the
        // value_enum must not make it a usage error.
        for fmt in ["JSON", "Json", "TEXT", "CSV", "SARIF", "Html"] {
            assert!(
                Args::try_parse_from(["ob", "scan", "-f", fmt, "."]).is_ok(),
                "format {fmt} must parse case-insensitively"
            );
        }
    }

    #[test]
    fn test_replace_list_presets_parses_without_dir() {
        assert!(Args::try_parse_from(["ob", "replace", "--list-presets"]).is_ok());
    }

    #[test]
    fn test_replace_accepts_positional_inputs_and_legacy_dir() {
        // Positional inputs mirror scan's grammar; -d stays a working legacy
        // alias, alone or combined with positional inputs.
        for (argv, positionals, has_dir) in [
            (
                &["ob", "replace", "src", "lib", "--preset", "clean-debug"][..],
                2,
                false,
            ),
            (
                &["ob", "replace", "-d", ".", "--preset", "clean-debug"],
                0,
                true,
            ),
            (
                &[
                    "ob",
                    "replace",
                    "src",
                    "-d",
                    "lib",
                    "--preset",
                    "clean-debug",
                ],
                1,
                true,
            ),
        ] {
            let args = Args::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            let Commands::Replace { inputs, dir, .. } = args.command else {
                panic!("{argv:?} must parse as replace");
            };
            assert_eq!(inputs.len(), positionals, "{argv:?} positional inputs");
            assert_eq!(dir.is_some(), has_dir, "{argv:?} legacy -d");
        }
    }

    #[test]
    fn test_replace_pattern_and_replacement_require_each_other() {
        use clap::error::ErrorKind;

        // -p alone used to become a null replacement, which deletes the line
        // after each match.
        for argv in [
            &["ob", "replace", ".", "-p", "TODO"][..],
            &["ob", "replace", ".", "-r", "DONE"],
        ] {
            let err = Args::try_parse_from(argv).expect_err("must be a usage error");
            assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument, "{argv:?}");
        }
        assert!(Args::try_parse_from(["ob", "replace", ".", "-p", "TODO", "-r", "DONE"]).is_ok());
    }

    #[test]
    fn test_replace_rule_sources_are_mutually_exclusive() {
        use clap::error::ErrorKind;

        // These used to be silently prioritized: preset over config over
        // pattern.
        for argv in [
            &[
                "ob",
                "replace",
                ".",
                "--preset",
                "clean-debug",
                "-c",
                "c.yaml",
            ][..],
            &[
                "ob",
                "replace",
                ".",
                "--preset",
                "clean-debug",
                "-p",
                "A",
                "-r",
                "B",
            ],
            &["ob", "replace", ".", "-c", "c.yaml", "-p", "A", "-r", "B"],
        ] {
            let err = Args::try_parse_from(argv).expect_err("must be a usage error");
            assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{argv:?}");
        }
    }

    #[test]
    fn test_preset_listing_names_every_preset_with_a_description() {
        let listing = preset_listing();
        for name in [
            "remove-copyright",
            "clean-debug",
            "remove-todos",
            "trim-whitespace",
            "remove-empty-comments",
            "tabs-to-spaces",
            "spaces-to-tabs",
        ] {
            assert!(
                listing.contains(name),
                "listing must name {name}: {listing}"
            );
        }
        // Descriptions come from the Preset doc comments.
        assert!(
            listing.contains("trailing whitespace"),
            "each line must carry the variant's doc comment: {listing}"
        );
    }
}
