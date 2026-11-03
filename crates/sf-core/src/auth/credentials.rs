//! Credential types with namespacing and lifecycle management.
//!
//! Credentials are namespaced by org and alias, with distinct operations
//! for local deletion vs. remote revocation.

use crate::auth::secret::Secret;
use std::time::SystemTime;

/// A credential stored for a specific org.
#[derive(Clone, Debug)]
pub struct Credential {
    /// Org ID (server-side identifier)
    pub org_id: String,
    /// User alias (local identifier)
    pub alias: String,
    /// Salesforce username associated with this credential.
    pub username: String,
    /// HTTPS instance URL associated with this credential.
    pub instance_url: String,
    /// Access token (secret)
    token: Secret,
    /// Refresh token (secret), when the selected OAuth flow provides one.
    refresh_token: Option<Secret>,
    /// Known access-token expiry, when supplied by the OAuth provider.
    expires_at: Option<SystemTime>,
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
            username: String::new(),
            instance_url: String::new(),
            token,
            refresh_token: None,
            expires_at: None,
            token_type,
            is_default: false,
        }
    }

    /// Attach the non-secret identity and instance details for this session.
    pub fn with_identity(
        mut self,
        username: impl Into<String>,
        instance_url: impl Into<String>,
    ) -> Self {
        self.username = username.into();
        self.instance_url = instance_url.into();
        self
    }

    /// Attach a refresh token without exposing it through formatting.
    pub fn with_refresh_token(mut self, refresh_token: Option<Secret>) -> Self {
        self.refresh_token = refresh_token;
        self
    }

    /// Set the known expiry of this access token.
    pub fn with_expiry(mut self, expires_at: Option<SystemTime>) -> Self {
        self.expires_at = expires_at;
        self
    }

    /// Get the access token (secret).
    pub fn token(&self) -> &Secret {
        &self.token
    }

    /// Get the refresh token, if this credential can refresh.
    pub fn refresh_token(&self) -> Option<&Secret> {
        self.refresh_token.as_ref()
    }

    /// Get the known access-token expiry.
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
    }

    pub(crate) fn update_tokens(
        &mut self,
        access_token: Secret,
        refresh_token: Option<Secret>,
        token_type: Option<String>,
        expires_at: Option<SystemTime>,
    ) {
        self.token = access_token;
        if let Some(refresh_token) = refresh_token {
            self.refresh_token = Some(refresh_token);
        }
        if let Some(token_type) = token_type {
            self.token_type = token_type;
        }
        self.expires_at = expires_at;
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
