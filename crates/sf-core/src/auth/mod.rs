//! Authentication and credential management.
//!
//! Provides credential storage with secret protection, OS adapter support,
//! and clear error handling when secure storage is unavailable.

pub mod credentials;
pub mod oauth;
pub mod secret;
pub mod session;
pub mod store;

pub use credentials::{Credential, CredentialKey, LogoutAction};
pub use oauth::{AuthorizationCode, OAuthFlow, OAuthTokens};
pub use secret::Secret;
pub use session::{AuthError, AuthProvider, OrgDirectory, SessionManager, TokenUpdate};
pub use store::{CredStoreError, CredStoreResult, CredentialBackend, InMemoryStore};
