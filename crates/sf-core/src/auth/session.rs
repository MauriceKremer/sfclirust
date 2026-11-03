//! Authenticated-session lifecycle and org selection.

use crate::auth::{
    CredStoreError, Credential, CredentialBackend, CredentialKey, LogoutAction, Secret,
};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

const REFRESH_SKEW: Duration = Duration::from_secs(60);
static REFRESH_LOCKS: OnceLock<Mutex<HashMap<CredentialKey, Weak<AsyncMutex<()>>>>> =
    OnceLock::new();

/// Authentication and org-resolution failures with safe, actionable messages.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The credential backend failed; secure-store errors are never hidden.
    #[error("Credential storage error: {0}")]
    CredentialStore(#[from] CredStoreError),
    /// The selected org does not have a stored credential.
    #[error("No authenticated org matches '{0}'")]
    OrgNotFound(String),
    /// The selection matches multiple stored orgs.
    #[error("Org selector '{0}' matches multiple authenticated orgs")]
    AmbiguousOrg(String),
    /// No default org is configured or stored.
    #[error("No default org is configured")]
    NoDefaultOrg,
    /// Multiple credentials were marked as the default.
    #[error("Multiple authenticated orgs are marked as default")]
    MultipleDefaultOrgs,
    /// A credential has expired and cannot be refreshed.
    #[error("The access token is expired and no refresh token is available; authenticate again")]
    ReauthenticationRequired,
    /// A refresh attempt failed. Provider response details are intentionally omitted.
    #[error("Access-token refresh failed; authenticate again if the session remains expired")]
    RefreshFailed,
    /// The OAuth provider could not revoke the token.
    #[error("Remote token revocation failed")]
    RevocationFailed,
    /// Remote revocation failed, but local credential deletion succeeded.
    #[error("Remote token revocation failed; local credentials were removed")]
    RevocationFailedAfterLocalDeletion,
    /// Remote revocation succeeded, but local credential deletion failed.
    #[error("Remote token revocation succeeded but local credential deletion failed: {0}")]
    LocalDeletionFailedAfterRevocation(CredStoreError),
    /// Both remote revocation and local credential deletion failed.
    #[error("Remote revocation and local credential deletion both failed: {0}")]
    LogoutPartialFailure(CredStoreError),
    /// The token provider returned an unusable or expired token update.
    #[error("The OAuth provider returned an invalid token update")]
    InvalidTokenUpdate,
    /// An internal synchronization lock was poisoned.
    #[error("Authentication synchronization is unavailable")]
    SynchronizationUnavailable,
    /// OAuth settings were invalid.
    #[error("Invalid OAuth configuration")]
    InvalidOAuthConfiguration,
    /// The local OAuth callback did not arrive before the deadline.
    #[error("OAuth login timed out waiting for the browser callback")]
    CallbackTimeout,
    /// The user or caller cancelled the OAuth login.
    #[error("OAuth login was cancelled")]
    LoginCancelled,
    /// The callback state did not match the state generated for this login.
    #[error("OAuth callback state validation failed")]
    CallbackStateMismatch,
    /// The callback request was invalid or exceeded the request limit.
    #[error("Invalid OAuth callback request")]
    InvalidCallback,
    /// Salesforce declined the OAuth authorization request.
    #[error("Salesforce declined OAuth authorization")]
    OAuthDenied,
    /// The OAuth token endpoint failed. Response bodies are not included.
    #[error("OAuth token request failed")]
    OAuthRequestFailed,
}

/// Tokens returned by a successful refresh.
pub struct TokenUpdate {
    /// New access token.
    pub access_token: Secret,
    /// Rotated refresh token, when the provider issues one.
    pub refresh_token: Option<Secret>,
    /// Token type if returned by the provider.
    pub token_type: Option<String>,
    /// Absolute access-token expiry, when known.
    pub expires_at: Option<SystemTime>,
}

/// OAuth operations used by the session lifecycle.
pub trait AuthProvider: Send + Sync {
    /// Refresh an access token once. Implementations must not retry.
    fn refresh<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<TokenUpdate, AuthError>> + Send + 'a>>;

    /// Revoke a credential remotely once.
    fn revoke<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<(), AuthError>> + Send + 'a>>;
}

/// Coordinates credential refresh and logout operations.
pub struct SessionManager<P> {
    store: Arc<dyn CredentialBackend>,
    provider: P,
}

impl<P: AuthProvider> SessionManager<P> {
    /// Create a session manager using the supplied credential backend and OAuth provider.
    pub fn new(store: Arc<dyn CredentialBackend>, provider: P) -> Self {
        Self { store, provider }
    }

    /// Return a usable access token, refreshing an expiring token once if needed.
    pub async fn access_token(&self, key: &CredentialKey) -> Result<Secret, AuthError> {
        let credential = self.store.retrieve(key)?;
        if !is_expiring(&credential, SystemTime::now()) {
            return Ok(credential.token().clone());
        }

        let refresh_lock = {
            let mut locks = REFRESH_LOCKS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .map_err(|_| AuthError::SynchronizationUnavailable)?;
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(key.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let _guard = refresh_lock.lock().await;

        // Re-read after acquiring the per-credential lock so waiters reuse a refresh.
        let mut credential = self.store.retrieve(key)?;
        let now = SystemTime::now();
        if !is_expiring(&credential, now) {
            return Ok(credential.token().clone());
        }
        if credential.refresh_token().is_none() {
            return Err(AuthError::ReauthenticationRequired);
        }

        let update = self.provider.refresh(&credential).await?;
        let refreshed_at = SystemTime::now();
        let minimum_expiry = refreshed_at
            .checked_add(REFRESH_SKEW)
            .unwrap_or(refreshed_at);
        if update.access_token.expose().is_empty() {
            return Err(AuthError::InvalidTokenUpdate);
        }
        let expires_at = match update.expires_at {
            Some(expiry) if expiry > minimum_expiry => Some(expiry),
            Some(_) => return Err(AuthError::InvalidTokenUpdate),
            None => None,
        };
        credential.update_tokens(
            update.access_token,
            update.refresh_token,
            update.token_type,
            expires_at,
        );
        self.store.store(credential.clone())?;
        Ok(credential.token().clone())
    }

    /// Log out locally, optionally attempting one remote revocation first.
    ///
    /// Local deletion is always attempted after remote revocation, including when
    /// revocation fails. The returned error distinguishes that partial outcome.
    pub async fn logout(&self, key: &CredentialKey, action: LogoutAction) -> Result<(), AuthError> {
        let revoke_result = match action {
            LogoutAction::LocalDeletion => None,
            LogoutAction::RemoteRevocation => {
                let credential = self.store.retrieve(key)?;
                Some(self.provider.revoke(&credential).await)
            }
        };
        let delete_result = self.store.delete(key);

        match (revoke_result, delete_result) {
            (None, Ok(())) => Ok(()),
            (None, Err(error)) => Err(AuthError::CredentialStore(error)),
            (Some(Ok(())), Ok(())) => Ok(()),
            (Some(Ok(())), Err(error)) => Err(AuthError::LocalDeletionFailedAfterRevocation(error)),
            (Some(Err(_)), Ok(())) => Err(AuthError::RevocationFailedAfterLocalDeletion),
            (Some(Err(_)), Err(error)) => Err(AuthError::LogoutPartialFailure(error)),
        }
    }
}

fn is_expiring(credential: &Credential, now: SystemTime) -> bool {
    let refresh_cutoff = now.checked_add(REFRESH_SKEW).unwrap_or(now);
    credential
        .expires_at()
        .is_some_and(|expiry| expiry <= refresh_cutoff)
}

/// Reads and resolves authenticated org metadata from the credential backend.
pub struct OrgDirectory {
    store: Arc<dyn CredentialBackend>,
}

impl OrgDirectory {
    /// Create an org directory backed by the supplied credential store.
    pub fn new(store: Arc<dyn CredentialBackend>) -> Self {
        Self { store }
    }

    /// List stored orgs in stable alias/username order.
    pub fn list(&self) -> Result<Vec<Credential>, AuthError> {
        let mut credentials = self.store.list()?;
        credentials.sort_by(|left, right| {
            left.alias
                .to_lowercase()
                .cmp(&right.alias.to_lowercase())
                .then_with(|| {
                    left.username
                        .to_lowercase()
                        .cmp(&right.username.to_lowercase())
                })
                .then_with(|| left.org_id.cmp(&right.org_id))
        });
        Ok(credentials)
    }

    /// Resolve an explicit selector, configured default, or stored default in that order.
    ///
    /// A selector matches aliases first, then usernames, then Salesforce org IDs.
    /// An invalid higher-precedence selector is an error and never falls back.
    pub fn resolve(
        &self,
        selector: Option<&str>,
        configured_default: Option<&str>,
    ) -> Result<Credential, AuthError> {
        let credentials = self.store.list()?;
        if let Some(selector) = selector {
            return resolve_selector(&credentials, selector);
        }
        if let Some(configured_default) = configured_default {
            return resolve_selector(&credentials, configured_default);
        }

        let defaults: Vec<_> = credentials
            .into_iter()
            .filter(|credential| credential.is_default)
            .collect();
        match defaults.len() {
            0 => Err(AuthError::NoDefaultOrg),
            1 => defaults.into_iter().next().ok_or(AuthError::NoDefaultOrg),
            _ => Err(AuthError::MultipleDefaultOrgs),
        }
    }
}

fn resolve_selector(credentials: &[Credential], selector: &str) -> Result<Credential, AuthError> {
    if selector.trim().is_empty() {
        return Err(AuthError::OrgNotFound(selector.to_owned()));
    }
    let selector_lower = selector.to_lowercase();
    for matches in [
        credentials
            .iter()
            .filter(|credential| credential.alias.to_lowercase() == selector_lower)
            .collect::<Vec<_>>(),
        credentials
            .iter()
            .filter(|credential| credential.username.to_lowercase() == selector_lower)
            .collect::<Vec<_>>(),
        credentials
            .iter()
            .filter(|credential| credential.org_id.to_lowercase() == selector_lower)
            .collect::<Vec<_>>(),
    ] {
        match matches.as_slice() {
            [credential] => return Ok((*credential).clone()),
            [] => {}
            _ => return Err(AuthError::AmbiguousOrg(selector.to_owned())),
        }
    }
    Err(AuthError::OrgNotFound(selector.to_owned()))
}
