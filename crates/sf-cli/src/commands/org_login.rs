//! `org login web`: browser OAuth login with secure persistence.
//!
//! Flow: validate flags, bind a loopback listener, print and (unless disabled)
//! open the authorization URL, wait for the state-validated callback, exchange
//! the code, look up the identity, then store the credential in the secure
//! store and the non-secret metadata in the org index.

use super::org::{open_store, storage_error};
use crate::cli::OrgLoginWebArgs;
use crate::error::{CommandError, CommandOutcome};
use serde_json::json;
use sf_core::auth::{AuthError, Credential, OAuthFlow};
use std::time::Duration;
use tokio::sync::oneshot;

const DEFAULT_INSTANCE_URL: &str = "https://login.salesforce.com";
const CLIENT_ID_ENV: &str = "SF_RUST_CLIENT_ID";
/// Test/automation override of the callback wait, in seconds.
const TIMEOUT_ENV: &str = "SF_RUST_LOGIN_TIMEOUT_SECS";
/// When set, the authorization URL is printed but no browser is launched.
/// Fixed loopback callback port for apps that register the port.
const CALLBACK_PORT_ENV: &str = "SF_RUST_CALLBACK_PORT";
/// Default loopback port, the same one the upstream CLI uses; the OAuth app
/// registers `http://localhost:1717/oauth/callback`.
const DEFAULT_CALLBACK_PORT: u16 = 1717;
const NO_BROWSER_ENV: &str = "SF_RUST_NO_BROWSER";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

fn auth_failure(error: &AuthError) -> CommandError {
    let code = match error {
        AuthError::CallbackTimeout => "LOGIN_TIMEOUT",
        AuthError::LoginCancelled => "LOGIN_CANCELLED",
        AuthError::OAuthDenied => "LOGIN_DENIED",
        AuthError::CallbackStateMismatch | AuthError::InvalidCallback => "INVALID_CALLBACK",
        _ => "AUTH_ERROR",
    };
    CommandError::operational(code, error.to_string())
}

fn login_timeout() -> Duration {
    std::env::var(TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs)
}

/// Best-effort browser launch without a shell; the URL is passed as one argument.
fn launch_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Flag first, then the environment, then the default port. Port 0 selects an
/// ephemeral port (used by tests).
fn resolve_callback_port(
    flag: Option<u16>,
    env_value: Option<String>,
) -> Result<u16, Box<CommandError>> {
    if let Some(port) = flag {
        return Ok(port);
    }
    match env_value {
        Some(value) => value.trim().parse::<u16>().map_err(|_| {
            Box::new(CommandError::parse_error(format!(
                "{CALLBACK_PORT_ENV} must be a port number from 0 to 65535"
            )))
        }),
        None => Ok(DEFAULT_CALLBACK_PORT),
    }
}

/// Run the browser login and persist the resulting org.
pub async fn web(args: OrgLoginWebArgs) -> CommandOutcome {
    match run_web(args).await {
        Ok(result) => CommandOutcome::Success {
            result: Some(result),
            warnings: vec![],
        },
        Err(error) => CommandOutcome::Failure(*error),
    }
}

async fn run_web(args: OrgLoginWebArgs) -> Result<serde_json::Value, Box<CommandError>> {
    let client_id = args
        .client_id
        .clone()
        .or_else(|| std::env::var(CLIENT_ID_ENV).ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            Box::new(
                CommandError::operational(
                    "MISSING_CLIENT_ID",
                    "An OAuth client ID is required for browser login",
                )
                .with_context(format!(
                    "Pass --client-id or set {CLIENT_ID_ENV}; the connected app must allow the \
                     http://localhost loopback callback with the api and refresh_token scopes"
                )),
            )
        })?;
    if args.alias.as_deref().is_some_and(|a| a.trim().is_empty()) {
        return Err(Box::new(CommandError::parse_error(
            "--alias must not be empty",
        )));
    }
    let instance_url = args.instance_url.as_deref().unwrap_or(DEFAULT_INSTANCE_URL);
    let callback_port =
        resolve_callback_port(args.callback_port, std::env::var(CALLBACK_PORT_ENV).ok())?;
    let flow = OAuthFlow::new(client_id, instance_url, login_timeout())
        .map_err(|_| {
            Box::new(CommandError::parse_error(
                "Invalid --instance-url or --client-id; the URL must be an https Salesforce login origin",
            ))
        })?
        .with_callback_port(callback_port);
    // Open storage before the browser step so a storage problem fails fast.
    let (mut store, index_path) = open_store()?;

    let attempt = flow
        .begin_login()
        .await
        .map_err(|e| Box::new(auth_failure(&e)))?;
    let url = attempt.authorization_url().to_string();
    eprintln!("Open this URL in a browser to authorize sf-rust:\n{url}");
    if std::env::var_os(NO_BROWSER_ENV).is_none() && !launch_browser(&url) {
        eprintln!("Could not launch a browser; open the URL above manually.");
    }

    // Interrupts are handled by the process entry point, which drops this future;
    // dropping `wait_for_callback` closes the loopback listener. The sender must
    // stay alive here because a dropped sender reads as cancellation.
    let (_keep_alive, cancel_rx) = oneshot::channel::<()>();
    let callback = attempt.wait_for_callback(cancel_rx).await;

    let code = callback.map_err(|e| Box::new(auth_failure(&e)))?;
    let tokens = flow
        .exchange_code(code)
        .await
        .map_err(|e| Box::new(auth_failure(&e)))?;
    let identity = flow
        .fetch_identity(&tokens)
        .await
        .map_err(|e| Box::new(auth_failure(&e)))?;
    let org_instance_url = tokens
        .instance_url
        .clone()
        .ok_or_else(|| Box::new(auth_failure(&AuthError::OAuthRequestFailed)))?;

    // One stored login per org and username: replace prior entries, keeping their
    // alias and default status unless the flags say otherwise.
    let previous: Vec<_> = store
        .list_orgs()
        .into_iter()
        .filter(|e| {
            e.org_id.eq_ignore_ascii_case(&identity.org_id)
                && e.username.eq_ignore_ascii_case(&identity.username)
        })
        .cloned()
        .collect();
    let was_default = previous.iter().any(|e| e.is_default);
    let alias = args
        .alias
        .clone()
        .or_else(|| previous.first().map(|e| e.alias.clone()))
        .unwrap_or_else(|| identity.username.clone());
    for old in &previous {
        store
            .remove_org(&old.org_id, &old.alias, &index_path)
            .map_err(|e| Box::new(storage_error(e)))?;
    }

    let credential = Credential::new(
        identity.org_id.clone(),
        alias.clone(),
        tokens.access_token,
        tokens.token_type,
    )
    .with_identity(&identity.username, &org_instance_url)
    .with_refresh_token(tokens.refresh_token)
    .with_expiry(tokens.expires_at);
    store
        .add_org(credential, &index_path)
        .map_err(|e| Box::new(storage_error(e)))?;
    let make_default = args.set_default || was_default;
    if make_default {
        store
            .set_default(&identity.org_id, &alias, &index_path)
            .map_err(|e| Box::new(storage_error(e)))?;
    }

    Ok(json!({
        "orgId": identity.org_id,
        "username": identity.username,
        "alias": alias,
        "instanceUrl": org_instance_url,
        "isDefaultUsername": make_default,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn callback_port_defaults_to_1717() {
        assert_eq!(resolve_callback_port(None, None).unwrap(), 1717);
    }

    #[test]
    fn callback_port_flag_beats_environment() {
        assert_eq!(
            resolve_callback_port(Some(2000), Some("3000".into())).unwrap(),
            2000
        );
        assert_eq!(
            resolve_callback_port(None, Some(" 3000 ".into())).unwrap(),
            3000
        );
        assert_eq!(resolve_callback_port(None, Some("0".into())).unwrap(), 0);
    }

    #[test]
    fn invalid_environment_port_is_an_error() {
        assert!(resolve_callback_port(None, Some("70000".into())).is_err());
        assert!(resolve_callback_port(None, Some("abc".into())).is_err());
    }
}
