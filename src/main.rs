//! The main entry point for the `oober` command-line application.
//!
//! This file is responsible for parsing command-line arguments and dispatching
//! to the appropriate subcommand handler in the `oober` library.

use oober::cli::{self, Commands};
use oober::errors::Result;
use oober::{file_renamer, replacer, scanner};
use std::env;
use std::process;

/// The main function of the application.
///
/// Errors from [`run`] are printed in their human-readable `Display` form;
/// returning `Result` from `main` would print the Rust `Debug` form
/// (e.g. `Error: RenameFailed(1)`).
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

/// Parses arguments and executes the corresponding command.
fn run() -> Result<()> {
    // Check if no arguments provided (just 'ob' or 'oober')
    let args_vec: Vec<String> = env::args().collect();
    if args_vec.len() == 1 {
        println!("⚡ Ultra-fast pattern scanner and code transformer\n");
        println!("QUICK START EXAMPLES:");
        println!("  ob scan .                              # Scan current directory");
        println!("  ob replace . -p 'TODO' -r 'TODO'       # Simple replacement");
        println!("  ob replace . --preset clean-debug      # Remove debug code");
        println!("  ob rename -d . -p 'test_(.*)' -r 'spec_$1'  # Rename files");
        println!("  ob undo -d .                           # Restore from backups\n");
        println!("Run 'ob --help' for full command list");
        println!("Run 'ob <command> --help' for detailed command help");
        process::exit(0);
    }

    // Check for specific commands with missing args and show examples.
    // Only the commands matched by name below are intercepted: `completions`
    // and `manpage` must fall through to clap, since `manpage` legitimately
    // takes no arguments and a bare `completions` gets clap's own
    // missing-<SHELL> error.
    if args_vec.len() == 2 {
        match args_vec[1].as_str() {
            "scan" => {
                eprintln!("Error: Missing required argument: <INPUTS>\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob scan .                              # Scan current directory");
                eprintln!("  ob scan -e 'TODO|FIXME' .              # Inline regex, no YAML file");
                eprintln!("  ob scan src/ lib/                      # Scan multiple directories");
                eprintln!("  ob scan -p patterns.yaml .             # Use custom patterns");
                eprintln!("  ob scan -x js,ts -o results.txt .      # Scan only JS/TS files");
                eprintln!("\nFor more options: ob scan --help");
                process::exit(1);
            }
            "replace" => {
                eprintln!("Error: Missing required argument: <INPUTS>\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob replace . --preset clean-debug          # Remove debug code");
                eprintln!("  ob replace . -p 'TODO' -r 'TODO'           # Simple replacement");
                eprintln!("  ob replace src/ lib/ -c config.yaml --dry-run  # Preview changes");
                eprintln!("  ob replace main.py --preset remove-todos   # Process a single file");
                eprintln!("  ob replace --list-presets                  # List available presets");
                eprintln!("\nFor more options: ob replace --help");
                process::exit(1);
            }
            "undo" => {
                eprintln!("Error: Missing required argument: --dir <DIR>\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob undo -d .                    # Restore all files");
                eprintln!("  ob undo -d src/ --keep-backups  # Restore but keep .bak files");
                eprintln!("\nFor more options: ob undo --help");
                process::exit(1);
            }
            "clean-backups" => {
                eprintln!("Error: Missing required argument: --dir <DIR>\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob clean-backups -d .            # Remove all backup files");
                eprintln!("  ob clean-backups -d . --dry-run  # Preview what would be deleted");
                eprintln!("\nFor more options: ob clean-backups --help");
                process::exit(1);
            }
            "rename" => {
                eprintln!("Error: Missing required arguments\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob rename -d . -p 'test_(.*)' -r 'spec_$1'      # Rename test files");
                eprintln!(
                    "  ob rename -d . -p '\\.tsx$' -r '.jsx' --dry-run  # Preview .tsx -> .jsx"
                );
                eprintln!(
                    "  ob rename -d . -p '^(\\d+)_(.+?)(\\.[^.]+)?$' -r '${{2}}_${{1}}${{3}}'  # 12_intro.md -> intro_12.md"
                );
                eprintln!("\nFor more options: ob rename --help");
                process::exit(1);
            }
            _ => {}
        }
    }

    let args = cli::parse_args();

    match args.command {
        Commands::Scan {
            patterns,
            regex,
            output,
            extensions,
            workers,
            incremental,
            force_full,
            clear_cache,
            content_hash,
            no_cache_content,
            format,
            include_summary,
            hidden,
            no_ignore,
            inputs,
        } => scanner::run_scan(scanner::ScanOptions {
            patterns_file: patterns,
            inline_patterns: regex,
            output,
            extensions,
            inputs,
            workers,
            incremental,
            force_full,
            clear_cache,
            content_hash,
            no_cache_content,
            format,
            include_summary,
            hidden,
            no_ignore,
        }),
        Commands::Replace {
            preset,
            list_presets,
            config,
            pattern,
            replacement,
            dir,
            extensions,
            exclude,
            no_backup,
            dry_run,
            verbose,
            workers,
            mut inputs,
        } => {
            // -d/--dir is a legacy alias for a positional input.
            inputs.extend(dir);
            if list_presets {
                print!("{}", cli::preset_listing());
                Ok(())
            } else if inputs.is_empty() {
                // Inputs are optional in clap only so that --list-presets can
                // run without one; an actual replace still needs at least one
                // (positional, or the legacy -d/--dir alias).
                Err("replace requires at least one input path (e.g. `ob replace . --preset clean-debug`; -d/--dir works as a legacy alias); run `ob replace --list-presets` to see the available presets".into())
            } else {
                replacer::run_replace(replacer::ReplaceOptions {
                    preset,
                    config_file: config,
                    pattern,
                    replacement,
                    inputs,
                    extensions,
                    exclude,
                    no_backup,
                    dry_run,
                    verbose,
                    workers,
                })
            }
        }
        Commands::Undo { dir, keep_backups } => replacer::run_undo(dir, keep_backups),
        Commands::CleanBackups { dir, dry_run } => replacer::run_clean_backups(dir, dry_run),
        Commands::Rename {
            dir,
            pattern,
            replacement,
            dry_run,
            verbose,
            workers,
        } => file_renamer::run_rename(dir, pattern, replacement, dry_run, verbose, workers),
        Commands::Completions { shell } => {
            cli::run_completions(shell);
            Ok(())
        }
        Commands::Manpage => cli::run_manpage(),
    }
}
