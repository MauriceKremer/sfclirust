//! Command-line interface definition using clap.
//!
//! Defines the root parser and subcommand tree for the sf-rust CLI.
//! Each subcommand is registered only when its implementation is complete.

use clap::{Parser, Subcommand};

/// Salesforce CLI reimplemented in Rust.
#[derive(Parser, Debug)]
#[command(
    name = "sf-rust",
    version = env!("CARGO_PKG_VERSION"),
    about = "Salesforce CLI reimplemented in Rust",
    long_about = "A native Rust reimplementation of the Salesforce CLI for improved performance and reliability.",
    propagate_version = true,
    subcommand_required = false,
)]
pub struct Cli {
    /// Subcommands, or None if no subcommand provided
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// All registered subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Org-related operations
    Org(OrgCommand),
}

/// Org subcommands
#[derive(Parser, Debug)]
pub struct OrgCommand {
    #[command(subcommand)]
    pub subcommand: OrgSubcommands,
}

/// Org subcommands
#[derive(Subcommand, Debug)]
pub enum OrgSubcommands {
    /// List all authenticated orgs
    List(OrgListArgs),
}

/// Arguments for `sf org list`
#[derive(Parser, Debug)]
pub struct OrgListArgs {
    /// Output results as JSON
    #[arg(long, global = false)]
    pub json: bool,

    /// Comma-separated list of columns to display
    #[arg(long)]
    pub columns: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli_parser() {
        // Verify that the CLI parser can be created
        let _cli = Cli::parse_from(["sf-rust", "--help"]);
    }
}
