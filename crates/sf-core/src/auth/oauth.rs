//! Salesforce authorization-code OAuth with PKCE and a loopback callback.

use crate::auth::session::{AuthError, AuthProvider, TokenUpdate};
use crate::auth::{Credential, Secret};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const CALLBACK_PATH: &str = "/oauth/callback";
const MAX_CALLBACK_BYTES: usize = 8 * 1024;
const MAX_TOKEN_RESPONSE_BYTES: usize = 16 * 1024;
const DEFAULT_SCOPES: &str = "api refresh_token";

/// A validated Salesforce browser authorization flow.
pub struct OAuthFlow {
    client_id: String,
    instance_url: reqwest::Url,
    timeout: Duration,
    client: reqwest::Client,
}

/// A pending browser-login attempt with a single-use loopback listener.
pub struct LoginAttempt {
    authorization_url: reqwest::Url,
    callback_url: reqwest::Url,
    client_id: String,
    state: Secret,
    code_verifier: Secret,
    listener: TcpListener,
    timeout: Duration,
}

/// Authorization code and PKCE verifier received from a validated callback.
pub struct AuthorizationCode {
    code: Secret,
    code_verifier: Secret,
    callback_url: reqwest::Url,
    client_id: String,
}

/// OAuth tokens and non-secret endpoint metadata returned after code exchange.
pub struct OAuthTokens {
    /// OAuth access token.
    pub access_token: Secret,
    /// OAuth refresh token, when issued.
    pub refresh_token: Option<Secret>,
    /// OAuth token type.
    pub token_type: String,
    /// Expiration if the provider reports it.
    pub expires_at: Option<SystemTime>,
    /// Salesforce instance URL returned by OAuth, if present.
    pub instance_url: Option<String>,
    /// Salesforce identity endpoint returned by OAuth, if present.
    pub identity_url: Option<String>,
}

impl OAuthFlow {
    /// Create an authorization-code flow for an HTTPS Salesforce login origin.
    pub fn new(
        client_id: impl Into<String>,
        instance_url: &str,
        timeout: Duration,
    ) -> Result<Self, AuthError> {
        let client_id = client_id.into();
        if client_id.trim().is_empty() || timeout.is_zero() {
            return Err(AuthError::InvalidOAuthConfiguration);
        }
        let instance_url = secure_base_url(instance_url)?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AuthError::InvalidOAuthConfiguration)?;
        Ok(Self {
            client_id,
            instance_url,
            timeout,
            client,
        })
    }

    /// Bind an ephemeral listener to IPv4 loopback and create a PKCE challenge.
    pub async fn begin_login(&self) -> Result<LoginAttempt, AuthError> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .map_err(|_| AuthError::InvalidOAuthConfiguration)?;
        let local_addr = listener
            .local_addr()
            .map_err(|_| AuthError::InvalidOAuthConfiguration)?;
        let callback_url = reqwest::Url::parse(&format!(
            "http://127.0.0.1:{}{}",
            local_addr.port(),
            CALLBACK_PATH
        ))
        .map_err(|_| AuthError::InvalidOAuthConfiguration)?;

        let state = Secret::new(random_url_token()?);
        let code_verifier = Secret::new(random_url_token()?);
        let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, code_verifier.expose().as_bytes()));
        let mut authorization_url = self.instance_url.clone();
        authorization_url.set_path("/services/oauth2/authorize");
        authorization_url.set_query(None);
        authorization_url.set_fragment(None);
        authorization_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", callback_url.as_str())
            .append_pair("scope", DEFAULT_SCOPES)
            .append_pair("state", state.expose())
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");

        Ok(LoginAttempt {
            authorization_url,
            callback_url,
            client_id: self.client_id.clone(),
            state,
            code_verifier,
            listener,
            timeout: self.timeout,
        })
    }

    /// Exchange a state-validated authorization code for OAuth tokens.
    pub async fn exchange_code(
        &self,
        authorization_code: AuthorizationCode,
    ) -> Result<OAuthTokens, AuthError> {
        if authorization_code.client_id != self.client_id
            || !is_loopback_callback(&authorization_code.callback_url)
        {
            return Err(AuthError::InvalidOAuthConfiguration);
        }
        let endpoint = endpoint(&self.instance_url, "/services/oauth2/token");
        let response = self
            .client
            .post(endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", authorization_code.code.expose()),
                ("client_id", authorization_code.client_id.as_str()),
                ("redirect_uri", authorization_code.callback_url.as_str()),
                ("code_verifier", authorization_code.code_verifier.expose()),
            ])
            .send()
            .await
            .map_err(|_| AuthError::OAuthRequestFailed)?;
        parse_token_response(response).await
    }
}

impl LoginAttempt {
    /// Get the URL to open in the user's browser.
    pub fn authorization_url(&self) -> &reqwest::Url {
        &self.authorization_url
    }

    /// Get the loopback redirect URI registered for this login attempt.
    pub fn callback_url(&self) -> &reqwest::Url {
        &self.callback_url
    }

    /// Wait for one callback, enforcing the state value, timeout, and cancellation.
    ///
    /// Dropping this future closes the listener. A dropped cancellation sender is
    /// treated as cancellation rather than leaving a listener running in the background.
    pub async fn wait_for_callback(
        self,
        mut cancellation: oneshot::Receiver<()>,
    ) -> Result<AuthorizationCode, AuthError> {
        let callback = receive_callback(self.listener, self.state, self.timeout);
        tokio::select! {
            result = callback => result.map(|code| AuthorizationCode {
                code,
                code_verifier: self.code_verifier,
                callback_url: self.callback_url,
                client_id: self.client_id,
            }),
            _ = &mut cancellation => Err(AuthError::LoginCancelled),
        }
    }
}

impl AuthProvider for OAuthFlow {
    fn refresh<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<TokenUpdate, AuthError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let refresh_token = credential
                .refresh_token()
                .ok_or(AuthError::ReauthenticationRequired)?;
            let base_url = secure_base_url(&credential.instance_url)?;
            let response = self
                .client
                .post(endpoint(&base_url, "/services/oauth2/token"))
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.expose()),
                    ("client_id", self.client_id.as_str()),
                ])
                .send()
                .await
                .map_err(|_| AuthError::RefreshFailed)?;
            let tokens = parse_token_response(response)
                .await
                .map_err(|_| AuthError::RefreshFailed)?;
            Ok(TokenUpdate {
                access_token: tokens.access_token,
                refresh_token: tokens.refresh_token,
                token_type: Some(tokens.token_type),
                expires_at: tokens.expires_at,
            })
        })
    }

    fn revoke<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AuthError>> + Send + 'a>>
    {
        Box::pin(async move {
            let base_url = secure_base_url(&credential.instance_url)
                .map_err(|_| AuthError::RevocationFailed)?;
            let token = credential
                .refresh_token()
                .unwrap_or_else(|| credential.token());
            let response = self
                .client
                .post(endpoint(&base_url, "/services/oauth2/revoke"))
                .form(&[
                    ("token", token.expose()),
                    ("client_id", self.client_id.as_str()),
                ])
                .send()
                .await
                .map_err(|_| AuthError::RevocationFailed)?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(AuthError::RevocationFailed)
            }
        })
    }
}

async fn receive_callback(
    listener: TcpListener,
    expected_state: Secret,
    timeout: Duration,
) -> Result<Secret, AuthError> {
    let receive = async {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| AuthError::InvalidCallback)?;
        let request_target = read_request_target(&mut stream).await?;
        let callback_url = reqwest::Url::parse(&format!("http://127.0.0.1{request_target}"))
            .map_err(|_| AuthError::InvalidCallback)?;
        if callback_url.path() != CALLBACK_PATH {
            write_callback_response(&mut stream, false).await?;
            return Err(AuthError::InvalidCallback);
        }

        let mut parameters = std::collections::HashMap::new();
        for (key, value) in callback_url.query_pairs() {
            if parameters
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                write_callback_response(&mut stream, false).await?;
                return Err(AuthError::InvalidCallback);
            }
        }
        if parameters.get("state").map(String::as_str) != Some(expected_state.expose()) {
            write_callback_response(&mut stream, false).await?;
            return Err(AuthError::CallbackStateMismatch);
        }
        if parameters.contains_key("error") {
            write_callback_response(&mut stream, true).await?;
            return Err(AuthError::OAuthDenied);
        }
        let Some(code) = parameters.get("code").filter(|value| !value.is_empty()) else {
            write_callback_response(&mut stream, false).await?;
            return Err(AuthError::InvalidCallback);
        };
        write_callback_response(&mut stream, true).await?;
        Ok(Secret::new(code.clone()))
    };

    tokio::time::timeout(timeout, receive)
        .await
        .map_err(|_| AuthError::CallbackTimeout)?
}

async fn read_request_target(stream: &mut TcpStream) -> Result<String, AuthError> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| AuthError::InvalidCallback)?;
        if read == 0 || request.len() + read > MAX_CALLBACK_BYTES {
            return Err(AuthError::InvalidCallback);
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let request = std::str::from_utf8(&request).map_err(|_| AuthError::InvalidCallback)?;
    let request_line = request.lines().next().ok_or(AuthError::InvalidCallback)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(AuthError::InvalidCallback)?;
    let target = parts.next().ok_or(AuthError::InvalidCallback)?;
    let version = parts.next().ok_or(AuthError::InvalidCallback)?;
    if method != "GET"
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || parts.next().is_some()
        || !target.starts_with('/')
        || target.starts_with("//")
    {
        return Err(AuthError::InvalidCallback);
    }
    Ok(target.to_owned())
}

async fn write_callback_response(stream: &mut TcpStream, accepted: bool) -> Result<(), AuthError> {
    let (status, body) = if accepted {
        (
            "200 OK",
            "Authentication response received. You may close this window.",
        )
    } else {
        ("400 Bad Request", "Authentication response rejected.")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|_| AuthError::InvalidCallback)
}

async fn parse_token_response(response: reqwest::Response) -> Result<OAuthTokens, AuthError> {
    if !response.status().is_success() {
        return Err(AuthError::OAuthRequestFailed);
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AuthError::OAuthRequestFailed)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_TOKEN_RESPONSE_BYTES {
            return Err(AuthError::OAuthRequestFailed);
        }
        body.extend_from_slice(&chunk);
    }
    let token_response: TokenResponse =
        serde_json::from_slice(&body).map_err(|_| AuthError::OAuthRequestFailed)?;
    if token_response.access_token.is_empty() {
        return Err(AuthError::OAuthRequestFailed);
    }
    let expires_at = token_response
        .expires_in
        .and_then(|seconds| SystemTime::now().checked_add(Duration::from_secs(seconds)));
    let instance_url = token_response
        .instance_url
        .map(|url| secure_base_url(&url).map(|validated| validated.to_string()))
        .transpose()?;
    let identity_url = token_response
        .id
        .map(|url| secure_identity_url(&url))
        .transpose()?;
    Ok(OAuthTokens {
        access_token: Secret::new(token_response.access_token),
        refresh_token: token_response.refresh_token.map(Secret::new),
        token_type: token_response
            .token_type
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Bearer".to_owned()),
        expires_at,
        instance_url,
        identity_url,
    })
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    instance_url: Option<String>,
    id: Option<String>,
}

fn secure_base_url(value: &str) -> Result<reqwest::Url, AuthError> {
    let url = reqwest::Url::parse(value).map_err(|_| AuthError::InvalidOAuthConfiguration)?;
    if url.scheme() != "https"
        || !url.host_str().is_some_and(is_salesforce_host)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(AuthError::InvalidOAuthConfiguration);
    }
    Ok(url)
}

fn secure_identity_url(value: &str) -> Result<String, AuthError> {
    let url = reqwest::Url::parse(value).map_err(|_| AuthError::OAuthRequestFailed)?;
    if url.scheme() != "https"
        || !url.host_str().is_some_and(is_salesforce_host)
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/id/")
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(AuthError::OAuthRequestFailed);
    }
    Ok(url.to_string())
}

fn is_salesforce_host(host: &str) -> bool {
    host == "salesforce.com"
        || host.ends_with(".salesforce.com")
        || host == "salesforce.mil"
        || host.ends_with(".salesforce.mil")
        || host.ends_with(".force.com")
}

fn is_loopback_callback(url: &reqwest::Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == CALLBACK_PATH
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn endpoint(base_url: &reqwest::Url, path: &str) -> reqwest::Url {
    let mut url = base_url.clone();
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    url
}

fn random_url_token() -> Result<String, AuthError> {
    let mut random = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| AuthError::InvalidOAuthConfiguration)?;
    Ok(URL_SAFE_NO_PAD.encode(random))
}
