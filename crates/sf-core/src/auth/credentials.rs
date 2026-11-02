//! Credential types with namespacing and lifecycle management.
//!
//! Credentials are namespaced by org and alias, with distinct operations
//! for local deletion vs. remote revocation.

use crate::auth::secret::Secret;

/// A credential stored for a specific org.
#[derive(Clone, Debug)]
pub struct Credential {
    /// Org ID (server-side identifier)
    pub org_id: String,
    /// User alias (local identifier)
    pub alias: String,
    /// Access token (secret)
    token: Secret,
    /// Token type (e.g., "Bearer")
    pub token_type: String,
    /// Whether this is the default org
    pub is_default: bool,
}

impl Credential {
    /// Create a new credential.
    pub fn new(org_id: String, alias: String, token: Secret, token_type: String) -> Self {
        Credential {
            org_id,
            alias,
            token,
            token_type,
            is_default: false,
        }
    }

    /// Get the access token (secret).
    pub fn token(&self) -> &Secret {
        &self.token
    }

    /// Get the credential key for namespacing.
    pub fn key(&self) -> CredentialKey {
        CredentialKey::new(&self.org_id, &self.alias)
    }
}

/// A key for uniquely identifying a credential by org and alias.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CredentialKey {
    org_id: String,
    alias: String,
}

impl CredentialKey {
    /// Create a new credential key.
    pub fn new(org_id: &str, alias: &str) -> Self {
        CredentialKey {
            org_id: org_id.to_lowercase(),
            alias: alias.to_lowercase(),
        }
    }

    /// Get the org ID.
    pub fn org_id(&self) -> &str {
        &self.org_id
    }

    /// Get the alias.
    pub fn alias(&self) -> &str {
        &self.alias
    }
}

/// Logout action: distinguishes local deletion from remote revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoutAction {
    /// Delete credential locally but leave server tokens active
    LocalDeletion,
    /// Revoke credential on server (requires network)
    RemoteRevocation,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn credential_key_normalizes_case() {
        let key1 = CredentialKey::new("ORG123", "MyAlias");
        let key2 = CredentialKey::new("org123", "myalias");
        assert_eq!(key1, key2);
    }

    #[test]
    fn credential_key_hashes_consistently() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(CredentialKey::new("ORG123", "MyAlias"));
        assert!(set.contains(&CredentialKey::new("org123", "myalias")));
    }

    #[test]
    fn credential_new() {
        let token = Secret::new("access-token");
        let cred = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        );

        assert_eq!(cred.org_id, "org123");
        assert_eq!(cred.alias, "my-org");
        assert_eq!(cred.token_type, "Bearer");
        assert!(!cred.is_default);
    }

    #[test]
    fn credential_token_hides_secret() {
        let token = Secret::new("access-token");
        let cred = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        );

        let debug_str = format!("{:?}", cred.token());
        assert!(!debug_str.contains("access-token"));
        assert!(debug_str.contains("***"));
    }

    #[test]
    fn credential_key_from_credential() {
        let token = Secret::new("token");
        let cred = Credential::new(
            "ORG123".to_string(),
            "MyAlias".to_string(),
            token,
            "Bearer".to_string(),
        );

        let key = cred.key();
        assert_eq!(key.org_id(), "org123");
        assert_eq!(key.alias(), "myalias");
    }

    #[test]
    fn logout_action_variants() {
        assert_eq!(LogoutAction::LocalDeletion, LogoutAction::LocalDeletion);
        assert_eq!(
            LogoutAction::RemoteRevocation,
            LogoutAction::RemoteRevocation
        );
        assert_ne!(LogoutAction::LocalDeletion, LogoutAction::RemoteRevocation);
    }
}
