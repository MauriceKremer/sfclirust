mod cli;
mod commands;
mod diagnostics;
mod error;
mod output;
mod runtime;
mod terminal;
mod tracing_config;

use clap::{CommandFactory, Parser};
use cli::Cli;
use error::{CommandError, CommandOutcome, exit_code};
use output::render_outcome_as;
use runtime::flush_output;
use std::process;

#[allow(dead_code)]
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() {
    // Register termination signals before any work starts so none is lost.
    let mut signals = match TerminationSignals::register() {
        Ok(signals) => signals,
        Err(e) => {
            eprintln!("error: cannot register signal handlers: {}", e);
            process::exit(exit_code::OPERATIONAL_ERROR);
        }
    };

    // An interrupt drops the running command future, which releases its listeners
    // and connections; the process then reports the interruption and exits.
    // Commands that mutate remote state must record recovery information
    // themselves before they become interruptible.
    let (outcome, json) = tokio::select! {
        result = run() => result,
        signal = signals.next() => {
            runtime::request_shutdown();
            (
                interrupted_outcome(signal),
                std::env::args().any(|arg| arg == "--json"),
            )
        }
    };
    let exit_code = match render_outcome_as(&outcome, json) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: failed to render output: {}", e);
            exit_code::OPERATIONAL_ERROR
        }
    };

    // Ensure output is flushed before exit
    // A failed flush must not report success. A closed pipe is the reader's
    // choice (for example `| head`) and keeps the command's own exit code.
    let exit_code = match flush_output() {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe && exit_code == 0 => {
            eprintln!("error: failed to flush output: {}", e);
            exit_code::OPERATIONAL_ERROR
        }
        _ => exit_code,
    };

    process::exit(exit_code);
}

/// Process termination signals that interrupt a running command.
struct TerminationSignals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

/// Signal numbers used for the 128 + n exit code.
const SIGINT: i32 = 2;
#[cfg(unix)]
const SIGTERM: i32 = 15;

impl TerminationSignals {
    #[cfg(unix)]
    fn register() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    #[cfg(not(unix))]
    fn register() -> std::io::Result<Self> {
        Ok(Self {})
    }

    /// Wait for the next signal and return its number.
    #[cfg(unix)]
    async fn next(&mut self) -> i32 {
        tokio::select! {
            _ = self.interrupt.recv() => SIGINT,
            _ = self.terminate.recv() => SIGTERM,
        }
    }

    #[cfg(not(unix))]
    async fn next(&mut self) -> i32 {
        let _ = tokio::signal::ctrl_c().await;
        SIGINT
    }
}

fn interrupted_outcome(signal: i32) -> CommandOutcome {
    let mut error = CommandError::operational(
        "INTERRUPTED",
        "The command was interrupted before it completed",
    )
    .with_context(
        "Local waiting was cancelled; this does not roll back changes already accepted by a remote org"
            .to_string(),
    );
    error.exit_code = exit_code::signal(signal);
    CommandOutcome::Failure(error)
}

async fn run() -> (CommandOutcome, bool) {
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
                return (
                    CommandOutcome::Success {
                        result: None,
                        warnings: vec![],
                    },
                    false,
                );
            }

            // Otherwise it's a parse error (exit 2)
            return (
                CommandOutcome::Failure(CommandError::parse_error(e.to_string())),
                false,
            );
        }
    };

    // If no subcommand, show help (which clap would have done automatically)
    let json = cli.json;
    let outcome = match cli.command {
        Some(cmd) => commands::execute(cmd).await,
        None => {
            // Print help and exit with 0
            println!("{}", Cli::command().render_help());
            CommandOutcome::Success {
                result: None,
                warnings: vec![],
            }
        }
    };
    (outcome, json)
}
