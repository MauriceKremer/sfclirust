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
    /// Output results as a single JSON document
    #[arg(long, global = true)]
    pub json: bool,

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
    /// Authenticate to an org
    Login(OrgLoginCommand),
    /// Display details of an authenticated org (secrets are redacted)
    Display(OrgDisplayArgs),
    /// List all authenticated orgs
    List(OrgListArgs),
    /// Remove stored authentication for one or all orgs
    Logout(OrgLogoutArgs),
}

/// Login subcommands
#[derive(Parser, Debug)]
pub struct OrgLoginCommand {
    #[command(subcommand)]
    pub subcommand: OrgLoginSubcommands,
}

/// Registered login methods
#[derive(Subcommand, Debug)]
pub enum OrgLoginSubcommands {
    /// Authenticate using the browser (OAuth authorization code with PKCE)
    Web(OrgLoginWebArgs),
}

/// Arguments for the browser login
#[derive(Parser, Debug)]
pub struct OrgLoginWebArgs {
    /// Alias for the authenticated org (defaults to the username)
    #[arg(short = 'a', long, visible_alias = "setalias")]
    pub alias: Option<String>,

    /// Set the org as the default
    #[arg(short = 's', long, visible_alias = "setdefaultusername")]
    pub set_default: bool,

    /// OAuth client ID (or set SF_RUST_CLIENT_ID)
    #[arg(short = 'i', long, visible_alias = "clientid")]
    pub client_id: Option<String>,

    /// Salesforce login URL, for example https://test.salesforce.com
    #[arg(short = 'r', long, visible_alias = "instanceurl")]
    pub instance_url: Option<String>,

    /// Loopback callback port, default 1717 (or set SF_RUST_CALLBACK_PORT); the
    /// OAuth app must register http://localhost:<port>/oauth/callback
    #[arg(long, visible_alias = "callbackport")]
    pub callback_port: Option<u16>,
}

/// Arguments for `sf org display`
#[derive(Parser, Debug)]
pub struct OrgDisplayArgs {
    /// Org alias, username, or org ID (default: the org marked as default at login)
    #[arg(short = 'o', long, visible_alias = "targetusername")]
    pub target_org: Option<String>,

    /// API version to report instead of the org's latest
    #[arg(long)]
    pub api_version: Option<String>,
}

/// Arguments for `sf org list`
#[derive(Parser, Debug)]
pub struct OrgListArgs {
    /// Accepted for parity: native state tracks no scratch orgs, so nothing is added
    #[arg(long)]
    pub all: bool,

    /// Accepted for parity: connection status is never checked by this command
    #[arg(long, visible_alias = "skipconnectionstatus")]
    pub skip_connection_status: bool,
}

/// Arguments for `sf org logout`
#[derive(Parser, Debug)]
pub struct OrgLogoutArgs {
    /// Org alias or username to log out
    #[arg(
        short = 'o',
        long,
        visible_alias = "targetusername",
        conflicts_with = "all"
    )]
    pub target_org: Option<String>,

    /// Log out of all authenticated orgs
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Do not prompt for confirmation (required: prompts are not supported)
    #[arg(short = 'p', long, visible_alias = "noprompt")]
    pub no_prompt: bool,
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
