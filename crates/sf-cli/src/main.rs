mod error;
mod output;

use error::{CommandError, CommandOutcome, exit_code};
use output::render_outcome;
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let outcome = run();
    let exit_code = match render_outcome(&outcome) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: failed to render output: {}", e);
            exit_code::OPERATIONAL_ERROR
        }
    };
    process::exit(exit_code);
}

fn run() -> CommandOutcome {
    // Custom parsing to handle unknown commands with the correct exit code.
    let args: Vec<String> = std::env::args().collect();

    // Check for --help or --version first (these should succeed)
    if args.len() > 1 {
        match args[1].as_str() {
            "--help" | "-h" => {
                print_help();
                return CommandOutcome::Success {
                    result: None,
                    warnings: vec![],
                };
            }
            "--version" | "-V" => {
                println!("sf-rust {}", VERSION);
                return CommandOutcome::Success {
                    result: None,
                    warnings: vec![],
                };
            }
            _ => {}
        }
    }

    // If we got here with 1 or 2 args and the second is not a flag, treat it as an unknown command
    if args.len() > 1 && !args[1].starts_with('-') {
        return CommandOutcome::Failure(CommandError::unsupported(&args[1]));
    }

    // No subcommand: show help
    if args.len() == 1 {
        print_help();
        return CommandOutcome::Success {
            result: None,
            warnings: vec![],
        };
    }

    // Try to parse flags - this catches invalid flags like --invalid-flag
    // For now, we only support --help and --version, so anything else is a parse error
    if args.len() > 1 && args[1].starts_with('-') {
        return CommandOutcome::Failure(CommandError::parse_error(format!(
            "unknown flag: {}",
            args[1]
        )));
    }

    // No subcommand: show help
    print_help();
    CommandOutcome::Success {
        result: None,
        warnings: vec![],
    }
}

fn print_help() {
    println!("Usage: sf-rust [COMMAND] [OPTIONS]");
    println!();
    println!(
        "A native Rust reimplementation of the Salesforce CLI for improved performance and reliability."
    );
    println!();
    println!("OPTIONS:");
    println!("  -h, --help     Print help");
    println!("  -V, --version  Print version");
}
