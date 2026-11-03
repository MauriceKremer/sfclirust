//! Org-related commands (list, display, login, logout, etc.)

use crate::cli::{OrgCommand, OrgListArgs, OrgSubcommands};
use crate::error::{CommandError, CommandOutcome};

/// Execute an org subcommand.
pub async fn execute(cmd: OrgCommand) -> CommandOutcome {
    match cmd.subcommand {
        OrgSubcommands::List(args) => list(args),
    }
}

/// Return an explicit unsupported outcome until credential storage is wired end to end.
fn list(_args: OrgListArgs) -> CommandOutcome {
    CommandOutcome::Failure(CommandError::unsupported("sf org list"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_is_explicitly_unsupported_until_storage_is_wired() {
        let args = OrgListArgs {
            json: false,
            columns: None,
        };

        match list(args) {
            CommandOutcome::Failure(error) => {
                assert_eq!(error.code, "UNSUPPORTED_COMMAND");
                assert_eq!(error.exit_code, 127);
            }
            CommandOutcome::Success { .. } => {
                panic!("an unimplemented org list must not report success")
            }
        }
    }
}
