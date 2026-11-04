//! Credential storage interface with OS adapters.
//!
//! Provides a narrow interface for storing and retrieving credentials
//! from the OS secure store or in-memory test implementation.

use crate::auth::credentials::{Credential, CredentialKey};
use crate::auth::secret::Secret;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

/// What the OS secure store holds for one login: the whole session, so a later
/// process can refresh it. Stored as a single JSON secure-store item.
#[derive(Serialize, Deserialize)]
struct StoredPayload {
    access_token: String,
    refresh_token: Option<String>,
    /// Access-token expiry as seconds since the Unix epoch.
    expires_at: Option<u64>,
    token_type: String,
    username: String,
    instance_url: String,
}

/// Result type for credential store operations.
pub type CredStoreResult<T> = Result<T, CredStoreError>;

/// Errors that can occur in credential store operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredStoreError {
    /// Secure storage is unavailable (no OS keychain access, etc.)
    StorageUnavailable(String),
    /// Credential not found in storage
    NotFound(String),
    /// Failed to store credential
    StoreFailed(String),
    /// Failed to retrieve credential
    RetrieveFailed(String),
    /// Failed to delete credential
    DeleteFailed(String),
}

impl std::fmt::Display for CredStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredStoreError::StorageUnavailable(msg) => {
                write!(f, "Secure storage unavailable: {}", msg)
            }
            CredStoreError::NotFound(msg) => write!(f, "Credential not found: {}", msg),
            CredStoreError::StoreFailed(msg) => write!(f, "Failed to store credential: {}", msg),
            CredStoreError::RetrieveFailed(msg) => {
                write!(f, "Failed to retrieve credential: {}", msg)
            }
            CredStoreError::DeleteFailed(msg) => write!(f, "Failed to delete credential: {}", msg),
        }
    }
}

impl std::error::Error for CredStoreError {}

/// Platform-agnostic credential store interface.
pub trait CredentialBackend: Send + Sync {
    /// Store a credential.
    fn store(&self, credential: Credential) -> CredStoreResult<()>;

    /// Retrieve a credential by key.
    fn retrieve(&self, key: &CredentialKey) -> CredStoreResult<Credential>;

    /// List all stored credentials.
    fn list(&self) -> CredStoreResult<Vec<Credential>>;

    /// Delete a credential (local deletion only).
    fn delete(&self, key: &CredentialKey) -> CredStoreResult<()>;

    /// Check if credential exists.
    fn exists(&self, key: &CredentialKey) -> CredStoreResult<bool>;
}

/// In-memory test implementation of credential store.
///
/// Used for testing and ephemeral headless mode. Credentials are never persisted
/// and are lost when the process terminates.
#[derive(Debug)]
pub struct InMemoryStore {
    credentials: Arc<Mutex<HashMap<CredentialKey, Credential>>>,
}

impl InMemoryStore {
    /// Create a new in-memory store.
    pub fn new() -> Self {
        InMemoryStore {
            credentials: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialBackend for InMemoryStore {
    fn store(&self, credential: Credential) -> CredStoreResult<()> {
        let key = credential.key();
        let mut creds = self
            .credentials
            .lock()
            .map_err(|e| CredStoreError::StoreFailed(e.to_string()))?;
        creds.insert(key, credential);
        Ok(())
    }

    fn retrieve(&self, key: &CredentialKey) -> CredStoreResult<Credential> {
        let creds = self
            .credentials
            .lock()
            .map_err(|e| CredStoreError::RetrieveFailed(e.to_string()))?;
        creds
            .get(key)
            .cloned()
            .ok_or_else(|| CredStoreError::NotFound(format!("{:?}", key)))
    }

    fn list(&self) -> CredStoreResult<Vec<Credential>> {
        let creds = self
            .credentials
            .lock()
            .map_err(|e| CredStoreError::RetrieveFailed(e.to_string()))?;
        Ok(creds.values().cloned().collect())
    }

    fn delete(&self, key: &CredentialKey) -> CredStoreResult<()> {
        let mut creds = self
            .credentials
            .lock()
            .map_err(|e| CredStoreError::DeleteFailed(e.to_string()))?;
        creds
            .remove(key)
            .ok_or_else(|| CredStoreError::NotFound(format!("{:?}", key)))?;
        Ok(())
    }

    fn exists(&self, key: &CredentialKey) -> CredStoreResult<bool> {
        let creds = self
            .credentials
            .lock()
            .map_err(|e| CredStoreError::RetrieveFailed(e.to_string()))?;
        Ok(creds.contains_key(key))
    }
}

/// OS secure store implementation using the keyring crate.
///
/// Stores credentials in the platform's native credential storage:
/// - macOS: Keychain
/// - Windows: Credential Manager
/// - Linux: secret-service (via DBus)
///
/// Non-secret metadata (org ID, alias) is stored in plaintext via the index,
/// while the actual credential token is stored only in the OS secure store.
#[derive(Debug)]
pub struct OsSecureStore {
    service: String,
}

impl OsSecureStore {
    /// Create a new OS secure store with a service identifier.
    ///
    /// The service identifier should be a unique namespace to avoid conflicts.
    /// Example: "com.salesforce.sf-rust"
    pub fn new(service: impl Into<String>) -> Self {
        OsSecureStore {
            service: service.into(),
        }
    }

    /// Format a credential key for storage in the OS keyring.
    ///
    /// Lowercased like `CredentialKey`, so a credential stored with the org ID as
    /// Salesforce returned it is found again by a case-insensitive lookup.
    fn format_credential_key(org_id: &str, alias: &str) -> String {
        format!("sf-rust:{}:{}", org_id.to_lowercase(), alias.to_lowercase())
    }

    fn encode_payload(credential: &Credential) -> CredStoreResult<String> {
        let expires_at = credential
            .expires_at()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let payload = StoredPayload {
            access_token: credential.token().expose().to_string(),
            refresh_token: credential.refresh_token().map(|s| s.expose().to_string()),
            expires_at,
            token_type: credential.token_type.clone(),
            username: credential.username.clone(),
            instance_url: credential.instance_url.clone(),
        };
        serde_json::to_string(&payload)
            .map_err(|_| CredStoreError::StoreFailed("Failed to encode credential".to_string()))
    }

    fn decode_payload(key: &CredentialKey, raw: &str) -> CredStoreResult<Credential> {
        let payload: StoredPayload = serde_json::from_str(raw).map_err(|_| {
            CredStoreError::RetrieveFailed(
                "Stored credential is unreadable; log in again to replace it".to_string(),
            )
        })?;
        Ok(Credential::new(
            key.org_id().to_string(),
            key.alias().to_string(),
            Secret::new(payload.access_token),
            payload.token_type,
        )
        .with_identity(payload.username, payload.instance_url)
        .with_refresh_token(payload.refresh_token.map(Secret::new))
        .with_expiry(
            payload
                .expires_at
                .map(|secs| UNIX_EPOCH + Duration::from_secs(secs)),
        ))
    }
}

impl CredentialBackend for OsSecureStore {
    fn store(&self, credential: Credential) -> CredStoreResult<()> {
        let key = Self::format_credential_key(&credential.org_id, &credential.alias);
        let payload = Self::encode_payload(&credential)?;

        keyring::Entry::new(&self.service, &key)
            .and_then(|entry| entry.set_password(&payload))
            .map_err(|e| {
                CredStoreError::StoreFailed(format!("Failed to store credential: {}", e))
            })?;

        Ok(())
    }

    fn retrieve(&self, key: &CredentialKey) -> CredStoreResult<Credential> {
        let keyring_key = Self::format_credential_key(key.org_id(), key.alias());

        let raw = keyring::Entry::new(&self.service, &keyring_key)
            .and_then(|entry| entry.get_password())
            .map_err(|e| match e {
                keyring::error::Error::NoEntry => CredStoreError::NotFound(format!("{:?}", key)),
                e => {
                    CredStoreError::RetrieveFailed(format!("Failed to retrieve credential: {}", e))
                }
            })?;

        Self::decode_payload(key, &raw)
    }

    fn list(&self) -> CredStoreResult<Vec<Credential>> {
        Err(CredStoreError::StorageUnavailable(
            "OS keyring does not support listing credentials. Use a native org index instead."
                .to_string(),
        ))
    }

    fn delete(&self, key: &CredentialKey) -> CredStoreResult<()> {
        let keyring_key = Self::format_credential_key(key.org_id(), key.alias());

        keyring::Entry::new(&self.service, &keyring_key)
            .and_then(|entry| entry.delete_credential())
            .map_err(|e| match e {
                keyring::error::Error::NoEntry => CredStoreError::NotFound(format!("{:?}", key)),
                e => CredStoreError::DeleteFailed(format!("Failed to delete credential: {}", e)),
            })?;

        Ok(())
    }

    fn exists(&self, key: &CredentialKey) -> CredStoreResult<bool> {
        let keyring_key = Self::format_credential_key(key.org_id(), key.alias());

        match keyring::Entry::new(&self.service, &keyring_key)
            .and_then(|entry| entry.get_password())
        {
            Ok(_) => Ok(true),
            Err(keyring::error::Error::NoEntry) => Ok(false),
            Err(e) => Err(CredStoreError::RetrieveFailed(format!(
                "Failed to check credential existence: {}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn payload_round_trips_refresh_token_expiry_and_identity() {
        let expiry = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let cred = Credential::new(
            "00D1".to_string(),
            "dev".to_string(),
            Secret::new("access"),
            "Bearer".to_string(),
        )
        .with_identity("u@example.com", "https://x.my.salesforce.com")
        .with_refresh_token(Some(Secret::new("refresh")))
        .with_expiry(Some(expiry));
        let raw = OsSecureStore::encode_payload(&cred).unwrap();
        let back = OsSecureStore::decode_payload(&cred.key(), &raw).unwrap();
        assert_eq!(back.token().expose(), "access");
        assert_eq!(back.refresh_token().unwrap().expose(), "refresh");
        assert_eq!(back.expires_at(), Some(expiry));
        assert_eq!(back.username, "u@example.com");
        assert_eq!(back.instance_url, "https://x.my.salesforce.com");
    }

    #[test]
    fn unreadable_payload_is_an_error_not_a_token() {
        let key = CredentialKey::new("00D1", "dev");
        assert!(matches!(
            OsSecureStore::decode_payload(&key, "just-a-raw-token"),
            Err(CredStoreError::RetrieveFailed(_))
        ));
    }

    #[test]
    fn os_store_key_is_case_insensitive_like_credential_key() {
        // Store uses the org ID as returned by Salesforce; lookups go through a
        // lowercased CredentialKey. Both must produce the same keyring account.
        let stored = OsSecureStore::format_credential_key("00D0E0000000U0MUAU", "Dev-Live");
        let key = CredentialKey::new("00D0E0000000U0MUAU", "Dev-Live");
        let looked_up = OsSecureStore::format_credential_key(key.org_id(), key.alias());
        assert_eq!(stored, looked_up);
        assert_eq!(stored, "sf-rust:00d0e0000000u0muau:dev-live");
    }

    #[test]
    fn in_memory_store_store_and_retrieve() {
        let store = InMemoryStore::new();
        let token = Secret::new("my-token");
        let cred = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        );

        let key = cred.key();
        store.store(cred).expect("store failed");

        let retrieved = store.retrieve(&key).expect("retrieve failed");
        assert_eq!(retrieved.org_id, "org123");
        assert_eq!(retrieved.alias, "my-org");
    }

    #[test]
    fn in_memory_store_not_found() {
        let store = InMemoryStore::new();
        let key = CredentialKey::new("org123", "my-org");

        let result = store.retrieve(&key);
        assert!(result.is_err());
        assert!(matches!(result, Err(CredStoreError::NotFound(_))));
    }

    #[test]
    fn in_memory_store_list() {
        let store = InMemoryStore::new();

        for i in 0..3 {
            let token = Secret::new(format!("token-{}", i));
            let cred = Credential::new(
                format!("org{}", i),
                format!("alias{}", i),
                token,
                "Bearer".to_string(),
            );
            store.store(cred).expect("store failed");
        }

        let creds = store.list().expect("list failed");
        assert_eq!(creds.len(), 3);
    }

    #[test]
    fn in_memory_store_delete() {
        let store = InMemoryStore::new();
        let token = Secret::new("my-token");
        let cred = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        );

        let key = cred.key();
        store.store(cred).expect("store failed");
        assert!(store.exists(&key).expect("exists check failed"));

        store.delete(&key).expect("delete failed");
        assert!(!store.exists(&key).expect("exists check failed"));
    }

    #[test]
    fn in_memory_store_namespacing() {
        let store = InMemoryStore::new();

        // Store two credentials with same alias, different orgs
        let token1 = Secret::new("token-1");
        let cred1 = Credential::new(
            "org1".to_string(),
            "my-alias".to_string(),
            token1,
            "Bearer".to_string(),
        );

        let token2 = Secret::new("token-2");
        let cred2 = Credential::new(
            "org2".to_string(),
            "my-alias".to_string(),
            token2,
            "Bearer".to_string(),
        );

        store.store(cred1).expect("store 1 failed");
        store.store(cred2).expect("store 2 failed");

        // Both should exist with different keys
        let key1 = CredentialKey::new("org1", "my-alias");
        let key2 = CredentialKey::new("org2", "my-alias");

        assert!(store.exists(&key1).expect("exists 1 failed"));
        assert!(store.exists(&key2).expect("exists 2 failed"));

        let list = store.list().expect("list failed");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn credential_store_error_display() {
        let err = CredStoreError::StorageUnavailable("no keychain".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Secure storage unavailable"));
        assert!(!msg.contains("***"));
    }
}
