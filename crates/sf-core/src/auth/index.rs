//! Native org index for tracking credentials and their metadata.
//!
//! The org index stores non-secret metadata (org IDs, aliases, defaults) in a
//! dedicated persistent location, separate from the actual credential tokens
//! which are stored in the OS keyring.
//!
//! This allows:
//! - Listing orgs without accessing the keyring
//! - Cross-process coordination of credential state
//! - Tracking defaults and aliases without exposing secrets
//! - Detecting stale credentials that no longer exist in the keyring

use crate::auth::credentials::CredentialKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A lightweight entry in the org index (non-secret metadata).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrgIndexEntry {
    /// Org ID (server-side identifier)
    pub org_id: String,
    /// User alias (local name for this org)
    pub alias: String,
    /// Salesforce username associated with this org
    pub username: String,
    /// Instance URL associated with this org
    pub instance_url: String,
    /// Whether this is the default org
    pub is_default: bool,
}

impl OrgIndexEntry {
    /// Create a new index entry.
    pub fn new(
        org_id: impl Into<String>,
        alias: impl Into<String>,
        username: impl Into<String>,
        instance_url: impl Into<String>,
    ) -> Self {
        OrgIndexEntry {
            org_id: org_id.into(),
            alias: alias.into(),
            username: username.into(),
            instance_url: instance_url.into(),
            is_default: false,
        }
    }

    /// Set this org as the default.
    pub fn set_default(mut self, is_default: bool) -> Self {
        self.is_default = is_default;
        self
    }

    /// Get the credential key for this entry.
    pub fn key(&self) -> CredentialKey {
        CredentialKey::new(&self.org_id, &self.alias)
    }
}

/// Native org index: persistent mapping of orgs to their metadata.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OrgIndex {
    /// Map of credential keys to org index entries
    orgs: HashMap<String, OrgIndexEntry>,
    /// Current default org (if any)
    default_org: Option<String>,
}

impl OrgIndex {
    /// Create a new empty org index.
    pub fn new() -> Self {
        OrgIndex::default()
    }

    /// Add or update an org entry in the index.
    pub fn add_org(&mut self, entry: OrgIndexEntry) {
        let key_str = format!("{}:{}", entry.org_id, entry.alias).to_lowercase();
        if entry.is_default {
            self.default_org = Some(key_str.clone());
        }
        self.orgs.insert(key_str, entry);
    }

    /// Remove an org from the index.
    pub fn remove_org(&mut self, org_id: &str, alias: &str) -> Option<OrgIndexEntry> {
        let key_str = format!("{}:{}", org_id, alias).to_lowercase();
        let entry = self.orgs.remove(&key_str);

        // Clear default if this was the default org
        if self.default_org == Some(key_str) {
            self.default_org = None;
        }

        entry
    }

    /// Get an org entry by org ID and alias.
    pub fn get_org(&self, org_id: &str, alias: &str) -> Option<&OrgIndexEntry> {
        let key_str = format!("{}:{}", org_id, alias).to_lowercase();
        self.orgs.get(&key_str)
    }

    /// List all org entries.
    pub fn list_orgs(&self) -> Vec<&OrgIndexEntry> {
        self.orgs.values().collect()
    }

    /// Get the default org entry, if one is set.
    pub fn default_org(&self) -> Option<&OrgIndexEntry> {
        self.default_org.as_ref().and_then(|key| self.orgs.get(key))
    }

    /// Set a different org as the default.
    pub fn set_default(&mut self, org_id: &str, alias: &str) -> bool {
        let key_str = format!("{}:{}", org_id, alias).to_lowercase();
        if self.orgs.contains_key(&key_str) {
            self.default_org = Some(key_str);

            // Mark the new default in all entries
            for entry in self.orgs.values_mut() {
                entry.is_default = false;
            }
            if let Some(entry) = self
                .orgs
                .get_mut(&format!("{}:{}", org_id, alias).to_lowercase())
            {
                entry.is_default = true;
            }

            true
        } else {
            false
        }
    }

    /// Load the index from a JSON file.
    pub fn load_from_file(path: &Path) -> std::io::Result<Self> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)?;
            serde_json::from_str(&contents).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse org index: {}", e),
                )
            })
        } else {
            Ok(OrgIndex::new())
        }
    }

    /// Save the index to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize org index: {}", e),
            )
        })?;

        std::fs::write(path, contents)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn org_index_add_and_get() {
        let mut index = OrgIndex::new();
        let entry = OrgIndexEntry::new(
            "org123",
            "my-org",
            "user@example.com",
            "https://example.my.salesforce.com",
        );

        index.add_org(entry.clone());

        let retrieved = index.get_org("org123", "my-org").expect("org not found");
        assert_eq!(retrieved, &entry);
    }

    #[test]
    fn org_index_default_org() {
        let mut index = OrgIndex::new();

        let entry1 = OrgIndexEntry::new(
            "org1",
            "alias1",
            "user1@example.com",
            "https://example.my.salesforce.com",
        )
        .set_default(true);
        let entry2 = OrgIndexEntry::new(
            "org2",
            "alias2",
            "user2@example.com",
            "https://example.my.salesforce.com",
        );

        index.add_org(entry1);
        index.add_org(entry2);

        let default = index.default_org().expect("no default org");
        assert_eq!(default.org_id, "org1");
        assert_eq!(default.alias, "alias1");
    }

    #[test]
    fn org_index_set_default() {
        let mut index = OrgIndex::new();

        let entry1 = OrgIndexEntry::new(
            "org1",
            "alias1",
            "user1@example.com",
            "https://example.my.salesforce.com",
        );
        let entry2 = OrgIndexEntry::new(
            "org2",
            "alias2",
            "user2@example.com",
            "https://example.my.salesforce.com",
        );

        index.add_org(entry1);
        index.add_org(entry2);

        assert!(index.set_default("org2", "alias2"));
        let default = index.default_org().expect("no default org");
        assert_eq!(default.org_id, "org2");
    }

    #[test]
    fn org_index_remove_org() {
        let mut index = OrgIndex::new();
        let entry = OrgIndexEntry::new(
            "org123",
            "my-org",
            "user@example.com",
            "https://example.my.salesforce.com",
        );

        index.add_org(entry.clone());
        assert!(index.get_org("org123", "my-org").is_some());

        let removed = index.remove_org("org123", "my-org");
        assert_eq!(removed, Some(entry));
        assert!(index.get_org("org123", "my-org").is_none());
    }

    #[test]
    fn org_index_list_orgs() {
        let mut index = OrgIndex::new();

        for i in 0..3 {
            let entry = OrgIndexEntry::new(
                format!("org{}", i),
                format!("alias{}", i),
                format!("user{}@example.com", i),
                "https://example.my.salesforce.com",
            );
            index.add_org(entry);
        }

        let orgs = index.list_orgs();
        assert_eq!(orgs.len(), 3);
    }

    #[test]
    fn org_index_persist_to_file() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let index_path = temp_dir.path().join("org-index.json");

        let mut index = OrgIndex::new();
        let entry = OrgIndexEntry::new(
            "org123",
            "my-org",
            "user@example.com",
            "https://example.my.salesforce.com",
        )
        .set_default(true);

        index.add_org(entry);
        index.save_to_file(&index_path).expect("save failed");

        assert!(index_path.exists());

        let loaded = OrgIndex::load_from_file(&index_path).expect("load failed");
        assert_eq!(loaded.list_orgs().len(), 1);
        assert!(loaded.default_org().is_some());
    }

    #[test]
    fn org_index_case_insensitive() {
        let mut index = OrgIndex::new();

        let entry1 = OrgIndexEntry::new(
            "ORG123",
            "MyAlias",
            "user@example.com",
            "https://example.my.salesforce.com",
        );

        index.add_org(entry1);

        // Should find the same entry with different case
        let retrieved = index.get_org("org123", "myalias").expect("org not found");
        assert_eq!(retrieved.org_id, "ORG123");
    }

    #[test]
    fn org_index_remove_default_clears_default() {
        let mut index = OrgIndex::new();
        let entry = OrgIndexEntry::new(
            "org123",
            "my-org",
            "user@example.com",
            "https://example.my.salesforce.com",
        )
        .set_default(true);

        index.add_org(entry);
        assert!(index.default_org().is_some());

        index.remove_org("org123", "my-org");
        assert!(index.default_org().is_none());
    }
}
