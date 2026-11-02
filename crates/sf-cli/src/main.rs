mod cli;
mod commands;
mod error;
mod output;

use clap::{CommandFactory, Parser};
use cli::Cli;
use error::{CommandError, CommandOutcome, exit_code};
use output::render_outcome;
use std::process;

#[allow(dead_code)]
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() {
    let outcome = run().await;
    let exit_code = match render_outcome(&outcome) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: failed to render output: {}", e);
            exit_code::OPERATIONAL_ERROR
        }
    };
    process::exit(exit_code);
}

async fn run() -> CommandOutcome {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // Clap will handle --help and --version automatically
            // Any other error is a parse error (exit 2)
            if e.use_stderr() {
                eprintln!("{}", e);
            } else {
                println!("{}", e);
            }

            // Check if this is a help or version error (which should exit 0)
            if e.kind() == clap::error::ErrorKind::DisplayHelp
                || e.kind() == clap::error::ErrorKind::DisplayVersion
            {
                return CommandOutcome::Success {
                    result: None,
                    warnings: vec![],
                };
            }

            // Otherwise it's a parse error (exit 2)
            return CommandOutcome::Failure(CommandError::parse_error(e.to_string()));
        }
    };

    // If no subcommand, show help (which clap would have done automatically)
    match cli.command {
        Some(cmd) => commands::execute(cmd).await,
        None => {
            // Print help and exit with 0
            println!("{}", Cli::command().render_help());
            CommandOutcome::Success {
                result: None,
                warnings: vec![],
            }
        }
    }
}
