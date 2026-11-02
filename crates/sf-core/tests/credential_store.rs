//! Integration tests for credential storage and secret types.
//!
//! Tests verify namespacing, persistence lifecycle, unavailable-store behavior,
//! and no secret leakage in errors or debug output.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use sf_core::auth::{
    CredStoreError, Credential, CredentialBackend, CredentialKey, InMemoryStore, LogoutAction,
    Secret,
};

#[test]
fn secret_never_leaks_in_debug() {
    let secret = Secret::new("super-secret-token");
    let debug_output = format!("{:?}", secret);

    // Secret should never appear in debug output
    assert!(!debug_output.contains("super-secret-token"));
    assert!(!debug_output.contains("secret-token"));
    assert!(debug_output.contains("***"));
}

#[test]
fn secret_never_leaks_in_display() {
    let secret = Secret::new("super-secret-token");
    let display_output = format!("{}", secret);

    // Secret should never appear in display output
    assert!(!display_output.contains("super-secret-token"));
    assert!(!display_output.contains("secret-token"));
    assert!(display_output.contains("***"));
}

#[test]
fn secret_expose_intentional() {
    let secret = Secret::new("my-token");
    let exposed = secret.expose();

    assert_eq!(exposed, "my-token");
}

#[test]
fn credential_key_namespace_by_org_and_alias() {
    let key1 = CredentialKey::new("org123", "my-org");
    let key2 = CredentialKey::new("org456", "my-org");
    let key3 = CredentialKey::new("org123", "other-org");

    // Same org and alias should match
    assert_eq!(key1, CredentialKey::new("org123", "my-org"));

    // Different org should not match
    assert_ne!(key1, key2);

    // Different alias should not match
    assert_ne!(key1, key3);
}

#[test]
fn credential_key_normalization() {
    let key1 = CredentialKey::new("ORG123", "MyAlias");
    let key2 = CredentialKey::new("org123", "myalias");

    // Keys should normalize to lowercase
    assert_eq!(key1, key2);
}

#[test]
fn in_memory_store_basic_lifecycle() {
    let store = InMemoryStore::new();

    // Initially empty
    assert!(store.list().expect("list failed").is_empty());

    // Store credential
    let secret = Secret::new("access-token");
    let cred = Credential::new(
        "org123".to_string(),
        "my-org".to_string(),
        secret,
        "Bearer".to_string(),
    );
    let key = cred.key();

    store.store(cred).expect("store failed");

    // Can be retrieved
    let retrieved = store.retrieve(&key).expect("retrieve failed");
    assert_eq!(retrieved.org_id, "org123");
    assert_eq!(retrieved.alias, "my-org");

    // Can be deleted
    store.delete(&key).expect("delete failed");

    // Gone after delete
    assert!(store.retrieve(&key).is_err());
}

#[test]
fn in_memory_store_multiple_credentials() {
    let store = InMemoryStore::new();

    // Store multiple credentials
    for i in 0..5 {
        let secret = Secret::new(format!("token-{}", i));
        let cred = Credential::new(
            format!("org{}", i),
            format!("alias{}", i),
            secret,
            "Bearer".to_string(),
        );
        store.store(cred).expect("store failed");
    }

    // List returns all
    let creds = store.list().expect("list failed");
    assert_eq!(creds.len(), 5);

    // Can retrieve specific credential
    let key = CredentialKey::new("org2", "alias2");
    let cred = store.retrieve(&key).expect("retrieve failed");
    assert_eq!(cred.org_id, "org2");
}

#[test]
fn in_memory_store_namespacing_same_alias_different_orgs() {
    let store = InMemoryStore::new();

    // Store credentials with same alias but different orgs
    for i in 0..3 {
        let secret = Secret::new(format!("token-org{}", i));
        let cred = Credential::new(
            format!("org{}", i),
            "production".to_string(), // Same alias
            secret,
            "Bearer".to_string(),
        );
        store.store(cred).expect("store failed");
    }

    // All should exist
    let list = store.list().expect("list failed");
    assert_eq!(list.len(), 3);

    // Each should be independently retrievable
    for i in 0..3 {
        let key = CredentialKey::new(&format!("org{}", i), "production");
        let cred = store.retrieve(&key).expect("retrieve failed");
        assert_eq!(cred.org_id, format!("org{}", i));
    }
}

#[test]
fn in_memory_store_update_overwrites() {
    let store = InMemoryStore::new();

    // Store initial credential
    let secret1 = Secret::new("token-v1");
    let cred1 = Credential::new(
        "org123".to_string(),
        "my-org".to_string(),
        secret1,
        "Bearer".to_string(),
    );
    store.store(cred1).expect("store failed");

    // Overwrite with new credential
    let secret2 = Secret::new("token-v2");
    let cred2 = Credential::new(
        "org123".to_string(),
        "my-org".to_string(),
        secret2,
        "Bearer".to_string(),
    );
    store.store(cred2).expect("store failed");

    // Should have only one credential with new token
    let list = store.list().expect("list failed");
    assert_eq!(list.len(), 1);
}

#[test]
fn in_memory_store_exists_check() {
    let store = InMemoryStore::new();
    let key = CredentialKey::new("org123", "my-org");

    // Initially doesn't exist
    assert!(!store.exists(&key).expect("exists failed"));

    // After store, exists
    let secret = Secret::new("token");
    let cred = Credential::new(
        "org123".to_string(),
        "my-org".to_string(),
        secret,
        "Bearer".to_string(),
    );
    store.store(cred).expect("store failed");
    assert!(store.exists(&key).expect("exists failed"));

    // After delete, doesn't exist
    store.delete(&key).expect("delete failed");
    assert!(!store.exists(&key).expect("exists failed"));
}

#[test]
fn credential_store_error_displays_safely() {
    let errors = vec![
        CredStoreError::StorageUnavailable("no keychain access".to_string()),
        CredStoreError::NotFound("org123:my-org".to_string()),
        CredStoreError::StoreFailed("permission denied".to_string()),
        CredStoreError::RetrieveFailed("locked".to_string()),
        CredStoreError::DeleteFailed("not found".to_string()),
    ];

    for err in errors {
        let msg = format!("{}", err);
        // Error should be informative but never expose actual secrets
        assert!(!msg.is_empty());
        assert!(!msg.contains("***"));
    }
}

#[test]
fn logout_action_variants() {
    let local = LogoutAction::LocalDeletion;
    let remote = LogoutAction::RemoteRevocation;

    assert_ne!(local, remote);
    assert_eq!(local, LogoutAction::LocalDeletion);
    assert_eq!(remote, LogoutAction::RemoteRevocation);
}

#[test]
fn ephemeral_headless_credentials_not_persisted() {
    // Create a store for headless mode
    let store = InMemoryStore::new();

    // Add credentials
    let secret = Secret::new("ephemeral-token");
    let cred = Credential::new(
        "org123".to_string(),
        "headless".to_string(),
        secret,
        "Bearer".to_string(),
    );
    let key = cred.key();

    store.store(cred).expect("store failed");
    assert!(store.retrieve(&key).is_ok());

    // Create a new store (simulating process restart)
    let new_store = InMemoryStore::new();

    // Credentials are gone (never persisted)
    assert!(new_store.retrieve(&key).is_err());
}

#[test]
fn concurrent_credential_access() {
    let store = std::sync::Arc::new(InMemoryStore::new());
    let mut handles = vec![];

    // Spawn multiple threads that store and retrieve credentials
    for i in 0..5 {
        let store_clone = std::sync::Arc::clone(&store);
        let handle = std::thread::spawn(move || {
            let secret = Secret::new(format!("token-{}", i));
            let cred = Credential::new(
                format!("org{}", i),
                format!("alias{}", i),
                secret,
                "Bearer".to_string(),
            );
            let key = cred.key();

            store_clone.store(cred).expect("store failed");
            store_clone.retrieve(&key).expect("retrieve failed")
        });
        handles.push(handle);
    }

    // All threads should succeed
    for handle in handles {
        let result = handle.join();
        assert!(result.is_ok());
    }

    // All credentials should be present
    let list = store.list().expect("list failed");
    assert_eq!(list.len(), 5);
}

#[test]
fn secret_no_clone_if_not_needed() {
    // Secret can be cloned but should be used through references when possible
    let secret = Secret::new("my-token");
    let _secret_ref = &secret;
    let _secret_clone = secret.clone();

    // Both should hide their contents
    assert_eq!(format!("{:?}", _secret_ref), "Secret(***)");
    assert_eq!(format!("{:?}", _secret_clone), "Secret(***)");
}
