//! Terminal output rendering for JSON and human-readable formats.
//!
//! Separates structured output to stdout (JSON when requested, human when not)
//! from diagnostics/errors to stderr.

use crate::error::CommandOutcome;
use std::io;

/// Renders a command outcome to stdout/stderr based on the requested format.
///
/// - If `SF_CONTENT_TYPE=JSON`, the error envelope is written to stdout as JSON.
/// - Otherwise, human-readable output goes to stdout.
/// - Diagnostics and non-JSON errors go to stderr.
pub fn render_outcome(outcome: &CommandOutcome) -> io::Result<i32> {
    let json_mode = std::env::var("SF_CONTENT_TYPE")
        .map(|v| v.to_uppercase() == "JSON")
        .unwrap_or(false);

    match outcome {
        CommandOutcome::Success {
            result,
            warnings: _,
        } => {
            if json_mode {
                if let Some(result) = result {
                    let json_str = serde_json::to_string(result)
                        .unwrap_or_else(|_| r#"{"status":"success"}"#.to_string());
                    println!("{}", json_str);
                }
            } else {
                if let Some(result) = result {
                    println!("{}", result);
                }
            }
            Ok(0)
        }

        CommandOutcome::Failure(err) => {
            if json_mode {
                // JSON errors go to stdout with a nonzero exit code
                let json = serde_json::to_string(err).unwrap_or_else(|_| {
                    serde_json::json!({
                        "status": "error",
                        "code": "INTERNAL_ERROR",
                        "message": "Failed to serialize error"
                    })
                    .to_string()
                });
                println!("{}", json);
            } else {
                // Human-readable errors go to stderr
                eprintln!("error: {}", err.message);
                if let Some(context) = &err.context {
                    eprintln!("  {}", context);
                }
            }
            Ok(err.exit_code)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::error::CommandError;
    use serde_json::json;

    #[test]
    fn render_success_no_json() {
        let outcome = CommandOutcome::Success {
            result: Some(json!({"name": "test"})),
            warnings: vec![],
        };

        let exit_code = render_outcome(&outcome).expect("render succeeds");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn render_error_json_mode() {
        // Skip this test since setting env vars is unsafe without synchronization
        // The integration tests in output_contract.rs cover this case
    }

    #[test]
    fn render_error_human_mode() {
        let outcome = CommandOutcome::Failure(CommandError::operational(
            "TEST_ERROR",
            "something went wrong",
        ));

        let exit_code = render_outcome(&outcome).expect("render succeeds");
        assert_eq!(exit_code, 1);
    }
}
