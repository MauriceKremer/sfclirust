//! Org-related commands (list, display, login, logout, etc.)

use crate::cli::{OrgCommand, OrgListArgs, OrgSubcommands};
use crate::error::CommandOutcome;
use serde_json::json;

/// Execute an org subcommand.
pub async fn execute(cmd: OrgCommand) -> CommandOutcome {
    match cmd.subcommand {
        OrgSubcommands::List(args) => list(args).await,
    }
}

/// List all authenticated orgs.
///
/// Returns a table (default) or JSON array of org information.
/// For now, this returns an empty list since we don't have credential storage implemented yet.
/// This is a placeholder that will be replaced in B20 (state management).
async fn list(args: OrgListArgs) -> CommandOutcome {
    // Empty list for now - placeholder until credential store is implemented (B20)
    let orgs = Vec::<serde_json::Value>::new();

    if args.json {
        CommandOutcome::Success {
            result: Some(json!(orgs)),
            warnings: vec![],
        }
    } else {
        // Human-readable output: table format (placeholder)
        CommandOutcome::Success {
            result: Some(json!({
                "text": "No authenticated orgs found.",
                "format": "table"
            })),
            warnings: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_returns_success() {
        let args = OrgListArgs {
            json: false,
            columns: None,
        };

        let outcome = list(args).await;
        matches!(outcome, CommandOutcome::Success { .. });
    }

    #[tokio::test]
    async fn list_json_returns_array() {
        let args = OrgListArgs {
            json: true,
            columns: None,
        };

        let outcome = list(args).await;
        match outcome {
            CommandOutcome::Success { result, .. } => {
                assert!(result.as_ref().map(|r| r.is_array()).unwrap_or(false));
            }
            _ => panic!("Expected success"),
        }
    }
}
