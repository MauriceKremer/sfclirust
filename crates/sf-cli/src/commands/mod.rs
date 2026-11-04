//! Command implementations.
//!
//! Each module in this directory implements a command's logic,
//! taking parsed arguments and returning a CommandOutcome.

pub mod org;
pub mod org_display;
pub mod org_login;

use crate::cli::Commands;
use crate::error::CommandOutcome;

/// Execute a registered command and return the outcome.
pub async fn execute(command: Commands) -> CommandOutcome {
    match command {
        Commands::Org(org_cmd) => org::execute(org_cmd).await,
    }
}
