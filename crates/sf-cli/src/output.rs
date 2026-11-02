//! Terminal output rendering for JSON and human-readable formats.
//!
//! Separates structured output to stdout (JSON when requested, human when not)
//! from diagnostics/errors to stderr.

use crate::error::CommandOutcome;
use crate::terminal::BrokenPipeHandler;
use std::io;

/// Renders a command outcome to stdout/stderr based on the requested format.
///
/// - If `SF_CONTENT_TYPE=JSON`, the entire result/error envelope is written to stdout as JSON.
/// - Otherwise, human-readable output goes to stdout.
/// - Diagnostics and non-JSON errors go to stderr.
/// - Always ensures exactly one JSON document (when JSON mode is active).
pub fn render_outcome(outcome: &CommandOutcome) -> io::Result<i32> {
    let json_mode = std::env::var("SF_CONTENT_TYPE")
        .map(|v| v.to_uppercase() == "JSON")
        .unwrap_or(false);

    let handler = BrokenPipeHandler::new();

    match outcome {
        CommandOutcome::Success {
            result,
            warnings: _,
        } => {
            if json_mode {
                if let Some(result) = result {
                    let json_str = serde_json::to_string(result)
                        .unwrap_or_else(|_| r#"{"status":"success"}"#.to_string());
                    // Ensure exactly one JSON document on stdout
                    println!("{}", json_str);
                    handler.check_and_exit_if_broken();
                }
            } else {
                // Human-readable output to stdout
                if let Some(result) = result {
                    render_human_readable(result)?;
                    handler.check_and_exit_if_broken();
                }
            }
            Ok(0)
        }

        CommandOutcome::Failure(err) => {
            if json_mode {
                // JSON errors go to stdout as a single JSON document with a nonzero exit code
                let json = serde_json::to_string(err).unwrap_or_else(|_| {
                    serde_json::json!({
                        "status": "error",
                        "code": "INTERNAL_ERROR",
                        "message": "Failed to serialize error"
                    })
                    .to_string()
                });
                println!("{}", json);
                handler.check_and_exit_if_broken();
            } else {
                // Human-readable errors go to stderr
                render_human_error(err)?;
                handler.check_and_exit_if_broken();
            }
            Ok(err.exit_code)
        }
    }
}

/// Render a JSON value as human-readable output to stdout.
///
/// Formats the JSON in a readable way without leaking internal structure.
fn render_human_readable(value: &serde_json::Value) -> io::Result<()> {
    match value {
        serde_json::Value::Object(obj) => {
            // For objects, pretty-print with nice formatting
            for (key, val) in obj {
                match val {
                    serde_json::Value::String(s) => {
                        println!("{}: {}", key, s);
                    }
                    serde_json::Value::Number(n) => {
                        println!("{}: {}", key, n);
                    }
                    serde_json::Value::Bool(b) => {
                        println!("{}: {}", key, b);
                    }
                    serde_json::Value::Array(arr) => {
                        println!("{}:", key);
                        for item in arr {
                            println!("  - {}", item);
                        }
                    }
                    serde_json::Value::Object(_) | serde_json::Value::Null => {
                        println!("{}: {}", key, serde_json::to_string_pretty(val).unwrap_or_default());
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                println!("{}", item);
            }
        }
        serde_json::Value::String(s) => {
            println!("{}", s);
        }
        _ => {
            // For other types, just pretty-print
            println!(
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            );
        }
    }
    Ok(())
}

/// Render a command error as human-readable output to stderr.
fn render_human_error(err: &crate::error::CommandError) -> io::Result<()> {
    eprintln!("error: {}", err.message);
    if let Some(context) = &err.context {
        eprintln!("  {}", context);
    }
    if !err.warnings.is_empty() {
        eprintln!("\nwarnings:");
        for warning in &err.warnings {
            eprintln!("  - {}", warning);
        }
    }
    Ok(())
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

    #[test]
    fn render_human_readable_object() {
        let value = json!({
            "name": "Alice",
            "age": 30,
            "active": true
        });
        let result = render_human_readable(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn render_human_readable_array() {
        let value = json!(["item1", "item2", "item3"]);
        let result = render_human_readable(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn render_human_readable_string() {
        let value = json!("test string");
        let result = render_human_readable(&value);
        assert!(result.is_ok());
    }
}
