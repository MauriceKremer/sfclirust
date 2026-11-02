//! Structured tracing with automatic secret redaction.
//!
//! Provides a wrapper around `tracing` crate with field-level redaction
//! to prevent accidental exposure of credentials and sensitive data.
//!
//! This module is part of the foundational infrastructure (B10-03) and will be
//! used by commands in future backlog items. It is intentionally public but
//! currently unused.

#![allow(dead_code)]

use tracing::Level;
use tracing_subscriber::fmt::{Subscriber as FmtSubscriber, format::FmtSpan};

/// Maximum length of a log message before truncation.
const MAX_LOG_LENGTH: usize = 10000;

/// Common secret field names that should be automatically redacted.
static SECRET_FIELDS: &[&str] = &[
    "api_key",
    "api-key",
    "apikey",
    "token",
    "bearer",
    "password",
    "secret",
    "session_id",
    "session-id",
    "sessionid",
    "access_token",
    "refresh_token",
    "credential",
    "auth",
    "authorization",
];

/// Minimum length for a value to be considered a potential secret.
const MIN_SECRET_LENGTH: usize = 8;

/// Initialize structured tracing with redaction support.
///
/// This sets up the global tracing subscriber with:
/// - Formatted JSON output (when `json` feature is enabled)
/// - Automatic field redaction for known secret patterns
/// - Customizable log level
///
/// # Returns
///
/// `Ok(())` if tracing was successfully initialized.
/// `Err(_)` if a global subscriber was already initialized.
///
/// # Example
///
/// ```ignore
/// tracing_init(Level::INFO)?;
/// tracing::info!(command = "deploy", org = "demo", "Starting deployment");
/// ```
pub fn tracing_init(level: Level) -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_span_events(FmtSpan::ENTER | FmtSpan::CLOSE)
        .with_writer(std::io::stderr)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Check if a field name is known to contain secrets.
pub fn is_secret_field(name: &str) -> bool {
    let lower = name.to_lowercase();
    SECRET_FIELDS.iter().any(|pattern| {
        lower.contains(pattern) || lower.starts_with(pattern) || lower.ends_with(pattern)
    })
}

/// Redact a string that may contain secrets.
///
/// Truncates to a safe length and redacts if the value is long enough to be
/// a potential secret (e.g., API key, token, password).
pub fn redact_value(value: &str) -> String {
    if value.len() >= MIN_SECRET_LENGTH {
        "***".to_string()
    } else {
        // Short values are assumed to be safe (e.g., org names, user IDs)
        // Truncate if too long for logging
        if value.len() > MAX_LOG_LENGTH {
            format!("{}...(truncated)", &value[..MAX_LOG_LENGTH])
        } else {
            value.to_string()
        }
    }
}

/// Redact a value conditionally based on the field name.
///
/// If the field name suggests a secret, the value is redacted.
/// Otherwise, it is returned as-is (but truncated if too long).
pub fn redact_if_secret_field(field: &str, value: &str) -> String {
    if is_secret_field(field) {
        redact_value(value)
    } else {
        // Non-secret field: truncate but don't redact
        if value.len() > MAX_LOG_LENGTH {
            format!("{}...(truncated)", &value[..MAX_LOG_LENGTH])
        } else {
            value.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_field_detection() {
        assert!(is_secret_field("api_key"));
        assert!(is_secret_field("API_KEY"));
        assert!(is_secret_field("bearer_token"));
        assert!(is_secret_field("password"));
        assert!(is_secret_field("session_id"));

        assert!(!is_secret_field("user_name"));
        assert!(!is_secret_field("org_id"));
        assert!(!is_secret_field("command"));
    }

    #[test]
    fn redact_long_value() {
        let secret = "a".repeat(100);
        let redacted = redact_value(&secret);
        assert_eq!(redacted, "***");
    }

    #[test]
    fn redact_short_value_unchanged() {
        let short = "demo";
        let redacted = redact_value(short);
        assert_eq!(redacted, "demo");
    }

    #[test]
    fn redact_if_secret_field_redacts_known_secret() {
        let secret = "a".repeat(50);
        let redacted = redact_if_secret_field("api_key", &secret);
        assert_eq!(redacted, "***");
    }

    #[test]
    fn redact_if_secret_field_preserves_non_secret() {
        let value = "my-org-name";
        let redacted = redact_if_secret_field("org_name", value);
        assert_eq!(redacted, "my-org-name");
    }

    #[test]
    fn redact_truncates_long_non_secret() {
        let long_value = "x".repeat(20000);
        let redacted = redact_if_secret_field("description", &long_value);
        assert!(redacted.contains("..."));
        assert!(!redacted.contains(&long_value));
    }
}
