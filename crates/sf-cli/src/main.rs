use clap::{Parser, Subcommand};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "sf-rust",
    version = VERSION,
    about = "Salesforce CLI reimplemented in Rust",
    long_about = "A native Rust reimplementation of the Salesforce CLI for improved performance and reliability.",
    args_conflicts_with_subcommands = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Placeholder for future commands
    #[command(hide = true)]
    Internal,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Show help when no subcommand is provided
            println!("{}", <Cli as clap::CommandFactory>::command().render_help());
        }
        Some(Commands::Internal) => {
            eprintln!("This is an internal command placeholder");
            process::exit(1);
        }
    }
}
