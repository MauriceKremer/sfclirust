//! Typed secret wrappers that prevent accidental exposure in logs, debug output, or errors.
//!
//! Secrets are never serialized, logged, or exposed via Debug output.

use std::fmt;

/// A secret string that hides its contents from Debug, Display, and serialization.
///
/// Use this for tokens, passwords, and other sensitive data. The secret is
/// stored in memory as a regular String but is never exposed except through
/// controlled access methods.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Create a new secret from a string.
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// Access the secret value.
    ///
    /// This is the only way to read the secret. Use sparingly and never log
    /// the returned value.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Consume the secret and return its value.
    ///
    /// Use this when you need to move the secret (e.g., to pass to an API call).
    /// The secret is consumed and no longer available.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Check if this secret is equal to a string without exposing the secret.
    pub fn matches(&self, other: &str) -> bool {
        self.0 == other
    }
}

// Never expose the secret in Debug output
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

// Never expose the secret in Display output
impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

// Secrets are not serializable by default to prevent accidental exposure
// This ensures serde won't serialize secrets unless explicitly implemented
// and documented as safe.

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn secret_debug_hides_value() {
        let secret = Secret::new("my-secret-token");
        let debug_str = format!("{:?}", secret);
        assert!(!debug_str.contains("my-secret-token"));
        assert!(debug_str.contains("***"));
    }

    #[test]
    fn secret_display_hides_value() {
        let secret = Secret::new("my-secret-token");
        let display_str = format!("{}", secret);
        assert!(!display_str.contains("my-secret-token"));
        assert!(display_str.contains("***"));
    }

    #[test]
    fn secret_expose_returns_value() {
        let secret = Secret::new("my-secret-token");
        assert_eq!(secret.expose(), "my-secret-token");
    }

    #[test]
    fn secret_into_inner_consumes() {
        let secret = Secret::new("my-secret-token");
        let value = secret.into_inner();
        assert_eq!(value, "my-secret-token");
    }

    #[test]
    fn secret_matches_without_exposure() {
        let secret = Secret::new("my-secret-token");
        assert!(secret.matches("my-secret-token"));
        assert!(!secret.matches("wrong-token"));
    }
}
