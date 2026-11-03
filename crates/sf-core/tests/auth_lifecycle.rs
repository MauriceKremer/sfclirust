#![allow(clippy::expect_used, clippy::unwrap_used)]

use sf_core::auth::{
    AuthError, AuthProvider, Credential, CredentialBackend, CredentialKey, InMemoryStore,
    LogoutAction, OAuthFlow, OrgDirectory, Secret, SessionManager, TokenUpdate,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

#[derive(Clone)]
struct FakeProvider {
    refreshes: Arc<AtomicUsize>,
    revocations: Arc<AtomicUsize>,
    fail_refresh: bool,
    fail_revoke: bool,
    omit_expiry: bool,
    delay: Duration,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            refreshes: Arc::new(AtomicUsize::new(0)),
            revocations: Arc::new(AtomicUsize::new(0)),
            fail_refresh: false,
            fail_revoke: false,
            omit_expiry: false,
            delay: Duration::ZERO,
        }
    }
}

impl AuthProvider for FakeProvider {
    fn refresh<'a>(
        &'a self,
        _credential: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<TokenUpdate, AuthError>> + Send + 'a>> {
        Box::pin(async move {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            if self.fail_refresh {
                return Err(AuthError::RefreshFailed);
            }
            Ok(TokenUpdate {
                access_token: Secret::new("fresh-access-token"),
                refresh_token: None,
                token_type: Some("Bearer".to_owned()),
                expires_at: (!self.omit_expiry)
                    .then(|| SystemTime::now() + Duration::from_secs(3600)),
            })
        })
    }

    fn revoke<'a>(
        &'a self,
        _credential: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<(), AuthError>> + Send + 'a>> {
        Box::pin(async move {
            self.revocations.fetch_add(1, Ordering::SeqCst);
            if self.fail_revoke {
                Err(AuthError::RevocationFailed)
            } else {
                Ok(())
            }
        })
    }
}

fn credential(alias: &str, username: &str, org_id: &str, is_default: bool) -> Credential {
    let mut credential = Credential::new(
        org_id.to_owned(),
        alias.to_owned(),
        Secret::new("expired-access-token"),
        "Bearer".to_owned(),
    )
    .with_identity(username, "https://example.my.salesforce.com")
    .with_refresh_token(Some(Secret::new("refresh-token")))
    .with_expiry(Some(SystemTime::UNIX_EPOCH));
    credential.is_default = is_default;
    credential
}

async fn stored_session(store: &Arc<InMemoryStore>, credential: Credential) -> CredentialKey {
    let key = credential.key();
    store.store(credential).expect("store credential");
    key
}

#[tokio::test]
async fn refreshes_expired_access_token_and_persists_it() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let provider = FakeProvider::new();
    let sessions = SessionManager::new(store.clone(), provider.clone());

    let access_token = sessions.access_token(&key).await.expect("refresh succeeds");

    assert_eq!(access_token.expose(), "fresh-access-token");
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        store
            .retrieve(&key)
            .expect("credential remains stored")
            .token()
            .expose(),
        "fresh-access-token"
    );
}

#[tokio::test]
async fn refresh_failure_is_reported_without_retry() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let mut provider = FakeProvider::new();
    provider.fail_refresh = true;
    let sessions = SessionManager::new(store, provider.clone());

    assert!(matches!(
        sessions.access_token(&key).await,
        Err(AuthError::RefreshFailed)
    ));
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn refresh_without_provider_expiry_keeps_expiry_unknown() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let mut provider = FakeProvider::new();
    provider.omit_expiry = true;
    let sessions = SessionManager::new(store.clone(), provider);

    assert_eq!(
        sessions
            .access_token(&key)
            .await
            .expect("refresh without expiry succeeds")
            .expose(),
        "fresh-access-token"
    );
    assert_eq!(
        store
            .retrieve(&key)
            .expect("stored credential")
            .expires_at(),
        None
    );
}

#[tokio::test]
async fn expired_token_without_refresh_token_requires_authentication() {
    let store = Arc::new(InMemoryStore::new());
    let mut credential = credential("dev", "user@example.com", "00D1", false);
    credential = credential.with_refresh_token(None);
    let key = stored_session(&store, credential).await;
    let provider = FakeProvider::new();
    let sessions = SessionManager::new(store, provider.clone());

    assert!(matches!(
        sessions.access_token(&key).await,
        Err(AuthError::ReauthenticationRequired)
    ));
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_expiry_refreshes_only_once() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let mut provider = FakeProvider::new();
    provider.delay = Duration::from_millis(30);
    let provider = provider.clone();
    let first_sessions = SessionManager::new(store.clone(), provider.clone());
    let second_sessions = SessionManager::new(store, provider.clone());
    let first_key = key.clone();
    let second_key = key.clone();

    let (first, second) = tokio::join!(
        first_sessions.access_token(&first_key),
        second_sessions.access_token(&second_key)
    );

    assert_eq!(
        first.expect("first access token").expose(),
        "fresh-access-token"
    );
    assert_eq!(
        second.expect("second access token").expose(),
        "fresh-access-token"
    );
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_remote_revocation_still_deletes_local_credentials() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let mut provider = FakeProvider::new();
    provider.fail_revoke = true;
    let sessions = SessionManager::new(store.clone(), provider.clone());

    assert!(matches!(
        sessions.logout(&key, LogoutAction::RemoteRevocation).await,
        Err(AuthError::RevocationFailedAfterLocalDeletion)
    ));
    assert!(matches!(
        store.retrieve(&key),
        Err(sf_core::auth::CredStoreError::NotFound(_))
    ));
    assert_eq!(provider.revocations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn successful_remote_revocation_deletes_local_credentials() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let provider = FakeProvider::new();
    let sessions = SessionManager::new(store.clone(), provider.clone());

    sessions
        .logout(&key, LogoutAction::RemoteRevocation)
        .await
        .expect("remote logout succeeds");

    assert!(matches!(
        store.retrieve(&key),
        Err(sf_core::auth::CredStoreError::NotFound(_))
    ));
    assert_eq!(provider.revocations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn successful_remote_revocation_reports_local_delete_failure() {
    let store = Arc::new(DeleteFailStore {
        inner: InMemoryStore::new(),
    });
    let credential = credential("dev", "user@example.com", "00D1", false);
    let key = credential.key();
    store.inner.store(credential).expect("store credential");
    let provider = FakeProvider::new();
    let sessions = SessionManager::new(store.clone(), provider.clone());

    assert!(matches!(
        sessions.logout(&key, LogoutAction::RemoteRevocation).await,
        Err(AuthError::LocalDeletionFailedAfterRevocation(
            sf_core::auth::CredStoreError::DeleteFailed(_)
        ))
    ));
    assert!(store.inner.exists(&key).expect("credential remains"));
    assert_eq!(provider.revocations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn local_logout_does_not_attempt_remote_revocation() {
    let store = Arc::new(InMemoryStore::new());
    let key = stored_session(&store, credential("dev", "user@example.com", "00D1", false)).await;
    let provider = FakeProvider::new();
    let sessions = SessionManager::new(store.clone(), provider.clone());

    sessions
        .logout(&key, LogoutAction::LocalDeletion)
        .await
        .expect("local logout succeeds");

    assert!(matches!(
        store.retrieve(&key),
        Err(sf_core::auth::CredStoreError::NotFound(_))
    ));
    assert_eq!(provider.revocations.load(Ordering::SeqCst), 0);
}

#[test]
fn selector_precedence_is_explicit_and_never_falls_back_after_a_wrong_org() {
    let store = InMemoryStore::new();
    store
        .store(credential("prod", "prod@example.com", "00D1", true))
        .expect("store prod");
    store
        .store(credential("dev", "dev@example.com", "00D2", false))
        .expect("store dev");
    let orgs = OrgDirectory::new(Arc::new(store));

    assert_eq!(
        orgs.resolve(Some("dev@example.com"), Some("prod@example.com"))
            .expect("explicit username wins")
            .alias,
        "dev"
    );
    assert_eq!(
        orgs.resolve(None, Some("dev@example.com"))
            .expect("configured default wins")
            .alias,
        "dev"
    );
    assert_eq!(
        orgs.resolve(None, None)
            .expect("stored default is used if no configured default")
            .alias,
        "prod"
    );
    assert!(matches!(
        orgs.resolve(Some("wrong-org"), Some("prod@example.com")),
        Err(AuthError::OrgNotFound(_))
    ));
}

#[test]
fn multiple_stored_defaults_are_rejected() {
    let store = InMemoryStore::new();
    store
        .store(credential("one", "one@example.com", "00D1", true))
        .expect("store first default");
    store
        .store(credential("two", "two@example.com", "00D2", true))
        .expect("store second default");
    let orgs = OrgDirectory::new(Arc::new(store));

    assert!(matches!(
        orgs.resolve(None, None),
        Err(AuthError::MultipleDefaultOrgs)
    ));
}

#[test]
fn oauth_rejects_non_salesforce_and_non_https_origins() {
    assert!(matches!(
        OAuthFlow::new(
            "test-client",
            "http://login.salesforce.com",
            Duration::from_secs(1)
        ),
        Err(AuthError::InvalidOAuthConfiguration)
    ));
    assert!(matches!(
        OAuthFlow::new(
            "test-client",
            "https://attacker.example",
            Duration::from_secs(1)
        ),
        Err(AuthError::InvalidOAuthConfiguration)
    ));
}

#[tokio::test]
async fn oauth_callback_requires_matching_state() {
    let flow = OAuthFlow::new(
        "test-client",
        "https://login.salesforce.com",
        Duration::from_secs(1),
    )
    .expect("valid OAuth flow");
    let attempt = flow.begin_login().await.expect("begin login");
    let callback_url = attempt.callback_url().clone();
    let (cancel, cancellation) = oneshot::channel();
    let callback = tokio::spawn(attempt.wait_for_callback(cancellation));

    let mut stream = TcpStream::connect((
        callback_url.host_str().expect("callback host"),
        callback_url.port().expect("callback port"),
    ))
    .await
    .expect("connect to loopback callback");
    stream
        .write_all(
            b"GET /oauth/callback?code=auth-code&state=wrong HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        )
        .await
        .expect("send callback");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read callback response");

    drop(cancel);
    assert!(matches!(
        callback.await.expect("callback task"),
        Err(AuthError::CallbackStateMismatch)
    ));
}

#[tokio::test]
async fn oauth_callback_accepts_matching_state_and_returns_authorization_code() {
    let flow = OAuthFlow::new(
        "test-client",
        "https://login.salesforce.com",
        Duration::from_secs(1),
    )
    .expect("valid OAuth flow");
    let attempt = flow.begin_login().await.expect("begin login");
    let callback_url = attempt.callback_url().clone();
    let state = attempt
        .authorization_url()
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .expect("generated state");
    let (_cancel, cancellation) = oneshot::channel();
    let callback = tokio::spawn(attempt.wait_for_callback(cancellation));

    let mut stream = TcpStream::connect((
        callback_url.host_str().expect("callback host"),
        callback_url.port().expect("callback port"),
    ))
    .await
    .expect("connect to loopback callback");
    let request = format!(
        "GET /oauth/callback?code=auth-code&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("send callback");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read callback response");

    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200 OK"));
    assert!(callback.await.expect("callback task").is_ok());
}

#[tokio::test]
async fn oauth_callback_cancellation_and_timeout_are_bounded() {
    let cancelled_flow = OAuthFlow::new(
        "test-client",
        "https://login.salesforce.com",
        Duration::from_secs(1),
    )
    .expect("valid OAuth flow");
    let attempt = cancelled_flow
        .begin_login()
        .await
        .expect("begin cancelled login");
    let callback_url = attempt.callback_url().clone();
    let (cancel, cancellation) = oneshot::channel();
    cancel.send(()).expect("send cancellation");
    assert!(matches!(
        attempt.wait_for_callback(cancellation).await,
        Err(AuthError::LoginCancelled)
    ));
    assert!(
        TcpStream::connect((
            callback_url.host_str().expect("callback host"),
            callback_url.port().expect("callback port"),
        ))
        .await
        .is_err()
    );

    let timed_flow = OAuthFlow::new(
        "test-client",
        "https://login.salesforce.com",
        Duration::from_millis(20),
    )
    .expect("valid OAuth flow");
    let attempt = timed_flow.begin_login().await.expect("begin timed login");
    let (_cancel, cancellation) = oneshot::channel();
    assert!(matches!(
        attempt.wait_for_callback(cancellation).await,
        Err(AuthError::CallbackTimeout)
    ));
}

#[test]
fn oauth_attempt_uses_loopback_and_pkce_s256() {
    let flow = OAuthFlow::new(
        "test-client",
        "https://login.salesforce.com",
        Duration::from_secs(1),
    )
    .expect("valid OAuth flow");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create runtime");
    let attempt = runtime.block_on(flow.begin_login()).expect("begin login");
    let redirect = attempt.callback_url().host_str().expect("callback host");
    let query: std::collections::HashMap<_, _> = attempt
        .authorization_url()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    assert_eq!(redirect, "127.0.0.1");
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert!(query.contains_key("code_challenge"));
    assert!(query.contains_key("state"));
}

#[derive(Default)]
struct UnavailableStore;

impl CredentialBackend for UnavailableStore {
    fn store(&self, _credential: Credential) -> sf_core::auth::CredStoreResult<()> {
        Err(sf_core::auth::CredStoreError::StorageUnavailable(
            "test keychain unavailable".to_owned(),
        ))
    }

    fn retrieve(&self, _key: &CredentialKey) -> sf_core::auth::CredStoreResult<Credential> {
        Err(sf_core::auth::CredStoreError::StorageUnavailable(
            "test keychain unavailable".to_owned(),
        ))
    }

    fn list(&self) -> sf_core::auth::CredStoreResult<Vec<Credential>> {
        Err(sf_core::auth::CredStoreError::StorageUnavailable(
            "test keychain unavailable".to_owned(),
        ))
    }

    fn delete(&self, _key: &CredentialKey) -> sf_core::auth::CredStoreResult<()> {
        Err(sf_core::auth::CredStoreError::StorageUnavailable(
            "test keychain unavailable".to_owned(),
        ))
    }

    fn exists(&self, _key: &CredentialKey) -> sf_core::auth::CredStoreResult<bool> {
        Err(sf_core::auth::CredStoreError::StorageUnavailable(
            "test keychain unavailable".to_owned(),
        ))
    }
}

struct DeleteFailStore {
    inner: InMemoryStore,
}

impl CredentialBackend for DeleteFailStore {
    fn store(&self, credential: Credential) -> sf_core::auth::CredStoreResult<()> {
        self.inner.store(credential)
    }

    fn retrieve(&self, key: &CredentialKey) -> sf_core::auth::CredStoreResult<Credential> {
        self.inner.retrieve(key)
    }

    fn list(&self) -> sf_core::auth::CredStoreResult<Vec<Credential>> {
        self.inner.list()
    }

    fn delete(&self, _key: &CredentialKey) -> sf_core::auth::CredStoreResult<()> {
        Err(sf_core::auth::CredStoreError::DeleteFailed(
            "simulated failure".to_owned(),
        ))
    }

    fn exists(&self, key: &CredentialKey) -> sf_core::auth::CredStoreResult<bool> {
        self.inner.exists(key)
    }
}

#[test]
fn unavailable_credential_storage_is_not_reported_as_empty_success() {
    let orgs = OrgDirectory::new(Arc::new(UnavailableStore));
    assert!(matches!(
        orgs.list(),
        Err(AuthError::CredentialStore(
            sf_core::auth::CredStoreError::StorageUnavailable(_)
        ))
    ));
}
