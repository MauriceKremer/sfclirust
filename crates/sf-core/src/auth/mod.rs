//! Authentication and credential management.
//!
//! Provides credential storage with secret protection, OS adapter support,
//! and clear error handling when secure storage is unavailable.

pub mod credentials;
pub mod secret;
pub mod store;

pub use credentials::{Credential, CredentialKey, LogoutAction};
pub use secret::Secret;
pub use store::{CredStoreError, CredStoreResult, CredentialBackend, InMemoryStore};
