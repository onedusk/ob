//! The main entry point for the `oober` command-line application.
//!
//! This file is responsible for parsing command-line arguments and dispatching
//! to the appropriate subcommand handler in the `oober` library.

use oober::cli::{self, Commands};
use oober::errors::Result;
use oober::{replacer, scanner, file_renamer};
use std::env;
use std::process;

/// The main function of the application.
///
/// It parses arguments and executes the corresponding command.
fn main() -> Result<()> {
    // Check if no arguments provided (just 'ob' or 'oober')
    let args_vec: Vec<String> = env::args().collect();
    if args_vec.len() == 1 {
        println!("⚡ Ultra-fast pattern scanner and code transformer\n");
        println!("QUICK START EXAMPLES:");
        println!("  ob scan .                              # Scan current directory");
        println!("  ob replace -d . -p 'TODO' -r 'DONE'    # Simple replacement");
        println!("  ob replace -d . --preset CleanDebug    # Remove debug code");
        println!("  ob rename -d . -p 'test_(.*)' -r 'spec_$1'  # Rename files");
        println!("  ob undo -d .                           # Restore from backups\n");
        println!("Run 'ob --help' for full command list");
        println!("Run 'ob <command> --help' for detailed command help");
        process::exit(0);
    }

    // Check for specific commands with missing args and show examples
    if args_vec.len() == 2 {
        match args_vec[1].as_str() {
            "scan" => {
                eprintln!("Error: Missing required argument: <INPUTS>\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob scan .                              # Scan current directory");
                eprintln!("  ob scan src/ lib/                      # Scan multiple directories");
                eprintln!("  ob scan -p patterns.yaml .             # Use custom patterns");
                eprintln!("  ob scan -x js,ts -o results.txt .      # Scan only JS/TS files");
                eprintln!("\nFor more options: ob scan --help");
                process::exit(1);
            }
            "replace" => {
                eprintln!("Error: Missing required argument: --dir <DIR>\n");
                eprintln!("USAGE EXAMPLES:");
                eprintln!("  ob replace -d . --preset CleanDebug        # Remove debug code");
                eprintln!("  ob replace -d . -p 'TODO' -r 'DONE'        # Simple replacement");
                eprintln!("  ob replace -d . -c config.yaml --dry-run   # Preview changes");
                eprintln!("  ob replace -d src/ -x py --preset RemoveTodos");
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
                eprintln!("  ob rename -d . -p '\\.tsx$' -r '.jsx' --dry-run  # Preview .tsx -> .jsx");
                eprintln!("  ob rename -d . -p '(\\d+)_(.*)' -r '$2_$1'       # Reorder name parts");
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
            output,
            extensions,
            workers,
            inputs,
            ..  // ignore other new fields for now
        } => scanner::run_scan(patterns, output, extensions, inputs, workers),
        Commands::Replace {
            preset,
            config,
            pattern,
            replacement,
            dir,
            extensions,
            exclude,
            no_backup,
            dry_run,
            workers,
        } => replacer::run_replace(
            preset,
            config,
            pattern,
            replacement,
            dir,
            extensions,
            exclude,
            no_backup,
            dry_run,
            workers,
        ),
        Commands::Undo { dir, keep_backups } => replacer::run_undo(dir, keep_backups),
        Commands::CleanBackups { dir, dry_run } => replacer::run_clean_backups(dir, dry_run),
        Commands::Rename {
            dir,
            pattern,
            replacement,
            dry_run,
            workers,
        } => file_renamer::run_rename(dir, pattern, replacement, dry_run, workers),
    }
}
