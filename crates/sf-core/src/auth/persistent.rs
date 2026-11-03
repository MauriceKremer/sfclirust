//! Persistent session management with OS credential storage and org index.
//!
//! Coordinates credential storage in the OS keyring with metadata stored in
//! a native org index file, supporting:
//! - Multiple orgs with aliases and defaults
//! - Cross-process credential coordination
//! - Graceful fallback when OS storage is unavailable
//! - Explicit ephemeral mode for headless operations

use crate::auth::{
    AuthError, AuthProvider, Credential, CredentialBackend, CredentialKey, InMemoryStore, OrgIndex,
    OrgIndexEntry, OsSecureStore, SessionManager,
};
use std::path::Path;
use std::sync::Arc;

/// Manages persistent org credentials using OS secure storage and a local org index.
pub struct PersistentOrgStore {
    /// Credential storage backend (OS keyring or in-memory)
    backend: Arc<dyn CredentialBackend>,
    /// Native org metadata index
    index: OrgIndex,
}

impl PersistentOrgStore {
    /// Create a new persistent org store using OS secure storage.
    ///
    /// Returns `None` if the OS secure store is unavailable on this platform.
    pub fn with_os_backend(
        index_path: &Path,
        service_identifier: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let backend = Arc::new(OsSecureStore::new(service_identifier));
        let index = OrgIndex::load_from_file(index_path)?;

        Ok(PersistentOrgStore { backend, index })
    }

    /// Create a new persistent org store using in-memory storage (for testing or ephemeral mode).
    pub fn with_memory_backend(index_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let backend = Arc::new(InMemoryStore::new());
        let index = OrgIndex::load_from_file(index_path)?;

        Ok(PersistentOrgStore { backend, index })
    }

    /// Add or update an org in both the secure store and the index.
    pub fn add_org(
        &mut self,
        credential: Credential,
        index_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Store the credential token in the secure store
        self.backend.store(credential.clone())?;

        // Add metadata to the org index
        let entry = OrgIndexEntry::new(
            &credential.org_id,
            &credential.alias,
            &credential.username,
            &credential.instance_url,
        );
        self.index.add_org(entry);

        // Persist the updated index
        self.index.save_to_file(index_path)?;

        Ok(())
    }

    /// Get a credential by org ID and alias.
    pub fn get_credential(&self, org_id: &str, alias: &str) -> Result<Credential, AuthError> {
        let key = CredentialKey::new(org_id, alias);

        // Verify the org exists in the index
        if self.index.get_org(org_id, alias).is_none() {
            return Err(AuthError::OrgNotFound(format!("{}:{}", org_id, alias)));
        }

        // Retrieve from the credential backend
        self.backend
            .retrieve(&key)
            .map_err(AuthError::CredentialStore)
    }

    /// List all org metadata from the index.
    pub fn list_orgs(&self) -> Vec<&OrgIndexEntry> {
        self.index.list_orgs()
    }

    /// Get the default org, if one is configured.
    pub fn default_org(&self) -> Option<&OrgIndexEntry> {
        self.index.default_org()
    }

    /// Set a different org as the default.
    pub fn set_default(
        &mut self,
        org_id: &str,
        alias: &str,
        index_path: &Path,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let success = self.index.set_default(org_id, alias);
        if success {
            self.index.save_to_file(index_path)?;
        }
        Ok(success)
    }

    /// Remove an org from both the secure store and the index.
    pub fn remove_org(
        &mut self,
        org_id: &str,
        alias: &str,
        index_path: &Path,
    ) -> Result<Option<Credential>, Box<dyn std::error::Error>> {
        let key = CredentialKey::new(org_id, alias);

        // Remove from the index first
        self.index.remove_org(org_id, alias);

        // Remove from the secure store
        let result = match self.backend.delete(&key) {
            Ok(()) => None,
            Err(e) => {
                if matches!(e, crate::auth::CredStoreError::NotFound(_)) {
                    None
                } else {
                    return Err(Box::new(e));
                }
            }
        };

        // Persist the updated index
        self.index.save_to_file(index_path)?;

        Ok(result)
    }

    /// Reload the org index from disk.
    pub fn reload_index(&mut self, index_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.index = OrgIndex::load_from_file(index_path)?;
        Ok(())
    }

    /// Get the underlying credential backend for advanced operations.
    pub fn backend(&self) -> Arc<dyn CredentialBackend> {
        Arc::clone(&self.backend)
    }

    /// Create a session manager for authenticated operations.
    pub fn session_manager<P: AuthProvider>(&self, provider: P) -> SessionManager<P> {
        SessionManager::new(Arc::clone(&self.backend), provider)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::auth::Secret;
    use tempfile::TempDir;

    #[test]
    fn persistent_org_store_with_memory_backend() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let index_path = temp_dir.path().join("org-index.json");

        let mut store =
            PersistentOrgStore::with_memory_backend(&index_path).expect("store creation failed");

        let token = Secret::new("test-token");
        let credential = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        )
        .with_identity("user@example.com", "https://example.my.salesforce.com");

        store
            .add_org(credential, &index_path)
            .expect("add_org failed");

        let retrieved = store
            .get_credential("org123", "my-org")
            .expect("get_credential failed");
        assert_eq!(retrieved.org_id, "org123");
    }

    #[test]
    fn persistent_org_store_list_orgs() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let index_path = temp_dir.path().join("org-index.json");

        let mut store =
            PersistentOrgStore::with_memory_backend(&index_path).expect("store creation failed");

        for i in 0..3 {
            let token = Secret::new(format!("token-{}", i));
            let credential = Credential::new(
                format!("org{}", i),
                format!("alias{}", i),
                token,
                "Bearer".to_string(),
            )
            .with_identity(
                format!("user{}@example.com", i),
                "https://example.my.salesforce.com",
            );

            store
                .add_org(credential, &index_path)
                .expect("add_org failed");
        }

        let orgs = store.list_orgs();
        assert_eq!(orgs.len(), 3);
    }

    #[test]
    fn persistent_org_store_default_org() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let index_path = temp_dir.path().join("org-index.json");

        let mut store =
            PersistentOrgStore::with_memory_backend(&index_path).expect("store creation failed");

        let token = Secret::new("token");
        let credential = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        )
        .with_identity("user@example.com", "https://example.my.salesforce.com");

        store
            .add_org(credential, &index_path)
            .expect("add_org failed");
        store
            .set_default("org123", "my-org", &index_path)
            .expect("set_default failed");

        let default = store.default_org().expect("no default org");
        assert_eq!(default.org_id, "org123");
    }

    #[test]
    fn persistent_org_store_remove_org() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let index_path = temp_dir.path().join("org-index.json");

        let mut store =
            PersistentOrgStore::with_memory_backend(&index_path).expect("store creation failed");

        let token = Secret::new("token");
        let credential = Credential::new(
            "org123".to_string(),
            "my-org".to_string(),
            token,
            "Bearer".to_string(),
        )
        .with_identity("user@example.com", "https://example.my.salesforce.com");

        store
            .add_org(credential, &index_path)
            .expect("add_org failed");
        assert!(!store.list_orgs().is_empty());

        store
            .remove_org("org123", "my-org", &index_path)
            .expect("remove_org failed");

        assert_eq!(store.list_orgs().len(), 0);
    }

    #[test]
    fn persistent_org_store_reload_index() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let index_path = temp_dir.path().join("org-index.json");

        // Create and populate a store
        {
            let mut store = PersistentOrgStore::with_memory_backend(&index_path)
                .expect("store creation failed");

            let token = Secret::new("token");
            let credential = Credential::new(
                "org123".to_string(),
                "my-org".to_string(),
                token,
                "Bearer".to_string(),
            )
            .with_identity("user@example.com", "https://example.my.salesforce.com");

            store
                .add_org(credential, &index_path)
                .expect("add_org failed");
        }

        // Create a new store and reload the index
        let mut store =
            PersistentOrgStore::with_memory_backend(&index_path).expect("store creation failed");
        store
            .reload_index(&index_path)
            .expect("reload_index failed");

        let orgs = store.list_orgs();
        assert_eq!(orgs.len(), 1);
    }
}
