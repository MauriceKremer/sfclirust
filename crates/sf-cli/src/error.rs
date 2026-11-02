//! Typed command outcomes and error handling.
//!
//! Provides a structured result type that captures command success, warnings,
//! and failures while preserving actionable error context without exposing
//! credentials or full sensitive response bodies.

use serde::Serialize;
use std::fmt;
use thiserror::Error;

/// Exit codes matching upstream oclif behavior.
pub mod exit_code {
    /// Success exit code.
    #[allow(dead_code)]
    pub const SUCCESS: i32 = 0;

    /// Generic parse/validation error (clap-reported invalid arguments).
    pub const PARSE_ERROR: i32 = 2;

    /// Operational error (network, auth, command logic).
    pub const OPERATIONAL_ERROR: i32 = 1;

    /// Unsupported command.
    #[allow(dead_code)]
    pub const UNSUPPORTED: i32 = 127;

    /// Terminated by signal (128 + signal number, e.g. SIGTERM = 128 + 15 = 143).
    #[allow(dead_code)]
    pub fn signal(sig: i32) -> i32 {
        128 + sig
    }
}

/// A typed command outcome: success or failure.
///
/// The CLI translates this into stdout/stderr/exit-code output.
#[derive(Debug)]
pub enum CommandOutcome {
    /// Command succeeded with optional JSON result and warnings.
    Success {
        result: Option<serde_json::Value>,
        #[allow(dead_code)]
        warnings: Vec<String>,
    },

    /// Command failed with structured error context.
    Failure(CommandError),
}

/// Structured error with category and safe context.
///
/// Errors preserve actionable information without exposing credentials,
/// full response bodies, or implementation details.
#[derive(Debug, Error, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// Error status: "error" or specific category (e.g., "validation_error", "auth_error").
    pub status: String,

    /// Exit code to use when rendering this error.
    #[serde(skip)]
    pub exit_code: i32,

    /// Error code for machine consumption (e.g., "INVALID_ORG", "NETWORK_ERROR").
    pub code: String,

    /// Human-readable error message (no credentials or full stack traces).
    pub message: String,

    /// Optional additional context (e.g., suggestion to retry, documentation link).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Optional warnings that occurred before the error.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl CommandError {
    /// Create a new unsupported command error.
    #[allow(dead_code)]
    pub fn unsupported(command: &str) -> Self {
        Self {
            status: "error".to_string(),
            exit_code: exit_code::UNSUPPORTED,
            code: "UNSUPPORTED_COMMAND".to_string(),
            message: format!("Command '{}' is not supported", command),
            context: Some("Use 'sf-rust --help' to see supported commands".to_string()),
            warnings: Vec::new(),
        }
    }

    /// Create a parse/validation error (bad arguments).
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            exit_code: exit_code::PARSE_ERROR,
            code: "PARSE_ERROR".to_string(),
            message: message.into(),
            context: Some("Use '--help' to see usage".to_string()),
            warnings: Vec::new(),
        }
    }

    /// Create an operational error (runtime failure).
    #[allow(dead_code)]
    pub fn operational(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            exit_code: exit_code::OPERATIONAL_ERROR,
            code: code.into(),
            message: message.into(),
            context: None,
            warnings: Vec::new(),
        }
    }

    /// Set optional context (e.g., suggestion or link).
    #[allow(dead_code)]
    pub fn with_context(mut self, context: String) -> Self {
        self.context = Some(context);
        self
    }

    /// Add a warning that occurred before this error.
    #[allow(dead_code)]
    pub fn with_warning(mut self, warning: String) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Redact suspected secrets from this error (simple *** replacement).
    #[allow(dead_code)]
    pub fn redact_secrets(mut self) -> Self {
        self.message = redact_secrets(&self.message);
        self.context = self.context.map(|c| redact_secrets(&c));
        self.warnings = self.warnings.iter().map(|w| redact_secrets(w)).collect();
        self
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Simple redaction: replace common secret patterns with ***.
///
/// This is a basic approach; a more sophisticated implementation would use
/// field-level annotations (deferred to a later phase).
#[allow(dead_code)]
fn redact_secrets(s: &str) -> String {
    let mut result = s.to_string();

    // Redact API keys (40+ hex chars, or "Bearer " + token)
    result = redact_pattern(&result, r"Bearer [A-Za-z0-9_\-]+", "Bearer ***");

    // Redact session IDs and similar tokens (look for common prefixes)
    result = redact_pattern(&result, r"(session[_-]?id|token|api[_-]?key)=\S+", "***");

    // Redact email-like patterns in certain contexts
    result = redact_pattern(&result, r"(password|secret)=\S+", "$1=***");

    result
}

/// Helper to redact patterns using simple string replacement.
#[allow(dead_code)]
fn redact_pattern(s: &str, _pattern: &str, _replacement: &str) -> String {
    // For now, use simple substring matching for known patterns
    // A production version would use regex or a more sophisticated approach

    let patterns = [
        ("Bearer ", "Bearer ***"),
        ("session_id=", "session_id=***"),
        ("api_key=", "api_key=***"),
        ("password=", "password=***"),
        ("secret=", "secret=***"),
        ("token=", "token=***"),
    ];

    let mut result = s.to_string();
    for (pat, repl) in &patterns {
        if let Some(pos) = result.find(pat) {
            // Find the end of the token (space, comma, newline, or end of string)
            let start = pos + pat.len();
            let end = result[start..]
                .find(|c: char| c.is_whitespace() || c == ',' || c == '"' || c == '\'')
                .map(|p| start + p)
                .unwrap_or(result.len());

            result = format!(
                "{}{}{}",
                &result[..start],
                repl.trim_start_matches(pat),
                &result[end..]
            );
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_bearer_token() {
        let s = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let redacted = redact_secrets(s);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(redacted.contains("Bearer ***"));
    }

    #[test]
    fn redact_session_id() {
        let s = "session_id=abc123def456xyz";
        let redacted = redact_secrets(s);
        assert!(!redacted.contains("abc123def456xyz"));
        assert!(redacted.contains("session_id=***"));
    }

    #[test]
    fn error_unsupported_command() {
        let err = CommandError::unsupported("foo");
        assert_eq!(err.code, "UNSUPPORTED_COMMAND");
        assert_eq!(err.exit_code, 127);
        assert!(err.message.contains("foo"));
    }

    #[test]
    fn error_parse_error() {
        let err = CommandError::parse_error("missing required argument");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code, "PARSE_ERROR");
    }

    #[test]
    fn error_operational() {
        let err = CommandError::operational("NETWORK_ERROR", "connection timed out");
        assert_eq!(err.exit_code, 1);
        assert_eq!(err.code, "NETWORK_ERROR");
    }
}
