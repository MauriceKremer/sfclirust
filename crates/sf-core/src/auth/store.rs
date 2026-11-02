//! Credential storage interface with OS adapters.
//!
//! Provides a narrow interface for storing and retrieving credentials
//! from the OS secure store or in-memory test implementation.

use crate::auth::credentials::{Credential, CredentialKey};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::auth::secret::Secret;

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
