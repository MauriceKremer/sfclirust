//! `org display`: show details of one authenticated org.
//!
//! Local identity comes from the org index and secure store. A single read-only
//! request to `/services/data/` checks the connection and discovers the latest
//! API version. Access and refresh tokens are never printed.

use super::org::{open_store, resolve_target, storage_error};
use crate::cli::OrgDisplayArgs;
use crate::error::{CommandError, CommandOutcome};
use serde::Deserialize;
use serde_json::{Value, json};
use sf_api::{BearerToken, HttpClient, InstanceOrigin, Method, TransportSettings};
use sf_core::auth::{
    AuthError, CredStoreError, Credential, EphemeralCredential, OAuthFlow, OrgIndexEntry,
};
use std::time::Duration;

const CLIENT_ID_ENV: &str = "SF_RUST_CLIENT_ID";
const REDACTED: &str = "[REDACTED]";
const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

/// Outcome of the connection check.
#[derive(Debug, PartialEq, Eq)]
enum Connection {
    Connected { latest_version: Option<String> },
    Unavailable(String),
}

#[derive(Deserialize)]
struct VersionEntry {
    version: String,
}

/// Pick the highest numeric version from the `/services/data/` response.
fn latest_version(entries: Vec<VersionEntry>) -> Option<String> {
    entries
        .into_iter()
        .filter_map(|e| e.version.parse::<f64>().ok().map(|n| (n, e.version)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, version)| version)
}

/// Assemble the common display document; absent values are omitted and the
/// token is redacted. Callers add identity fields they know.
fn build_display(
    org_id: &str,
    instance_url: &str,
    api_version: Option<&str>,
    connected_status: &str,
) -> Value {
    let mut doc = json!({
        "id": org_id,
        "accessToken": REDACTED,
        "instanceUrl": instance_url,
        "connectedStatus": connected_status,
    });
    if let (Some(version), Some(map)) = (api_version, doc.as_object_mut()) {
        map.insert("apiVersion".to_string(), Value::String(version.to_string()));
    }
    doc
}

fn add_fields(doc: &mut Value, fields: &[(&str, Value)]) {
    if let Some(map) = doc.as_object_mut() {
        for (key, value) in fields {
            map.insert((*key).to_string(), value.clone());
        }
    }
}

fn valid_api_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let (major, minor) = (parts.next(), parts.next());
    parts.next().is_none()
        && major.is_some_and(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()))
        && minor.is_none_or(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()))
}

async fn check_connection(instance_url: &str, token: &str) -> Connection {
    let result = async {
        let origin = InstanceOrigin::parse(instance_url).map_err(|e| e.to_string())?;
        let mut settings = TransportSettings::default();
        if let Some(pem) = sf_core::tls::extra_ca_bundle(|name| std::env::var(name).ok())
            .map_err(|e| e.to_string())?
        {
            settings = settings.add_root_certificate_pem(pem);
        }
        let client = HttpClient::new(settings).map_err(|e| e.to_string())?;
        let url = origin
            .request_url("/services/data/")
            .map_err(|e| e.to_string())?;
        let bearer = BearerToken::new(token).map_err(|e| e.to_string())?;
        let response = client
            .execute(&origin, &bearer, Method::GET, &url, None)
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP status {}", response.status().as_u16()));
        }
        let versions: Vec<VersionEntry> = response.json().map_err(|e| e.to_string())?;
        Ok::<_, String>(latest_version(versions))
    }
    .await;
    match result {
        Ok(latest_version) => Connection::Connected { latest_version },
        Err(reason) => Connection::Unavailable(reason),
    }
}

/// Use the stored token, refreshing it first when a client ID is configured.
/// The refresh goes to the org's own instance URL, so sandboxes and My Domain
/// orgs refresh against the right host.
async fn usable_token(
    store: &sf_core::auth::PersistentOrgStore,
    credential: &Credential,
    instance_url: &str,
) -> Result<String, String> {
    let Some(client_id) = std::env::var(CLIENT_ID_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
    else {
        return Ok(credential.token().expose().to_string());
    };
    let flow =
        OAuthFlow::new(client_id, instance_url, REFRESH_TIMEOUT).map_err(|e| e.to_string())?;
    let manager = store.session_manager(flow);
    manager
        .access_token(&credential.key())
        .await
        .map(|secret| secret.expose().to_string())
        .map_err(|e| e.to_string())
}

/// Upstream `org display` error for an unknown target (a parse error, exit 2).
fn display_not_found(target: &str) -> CommandError {
    let mut error = CommandError::operational(
        "NamedOrgNotFoundError",
        format!(
            "Parsing --target-org \n\tNo authorization information found for {target}.\nSee more help with --help"
        ),
    );
    error.exit_code = 2;
    error
}

/// An explicit target wins; otherwise the org marked default at login is used.
fn select_entry(
    entries: &[OrgIndexEntry],
    target: Option<&str>,
) -> Result<OrgIndexEntry, Box<CommandError>> {
    match target {
        Some(target) => resolve_target(entries, target, display_not_found),
        None => entries
            .iter()
            .find(|e| e.is_default)
            .cloned()
            .ok_or_else(|| {
                Box::new(CommandError::operational(
                    "NoDefaultEnvError",
                    "No default environment found. Use -o or --target-org to specify an environment.",
                ))
            }),
    }
}

/// Display one org. A failed refresh or connection check warns and still shows
/// the local details.
pub async fn display(args: OrgDisplayArgs) -> CommandOutcome {
    if let Some(version) = &args.api_version
        && !valid_api_version(version)
    {
        return CommandOutcome::Failure(CommandError::parse_error(format!(
            "Invalid --api-version '{version}'; expected a value such as 61.0"
        )));
    }
    match EphemeralCredential::from_lookup(|name| std::env::var(name).ok()) {
        Err(error) => {
            return CommandOutcome::Failure(CommandError::operational(
                "EPHEMERAL_INPUT_INVALID",
                error.to_string(),
            ));
        }
        // Headless mode never opens the credential store or the org index.
        Ok(Some(ephemeral)) => return display_ephemeral(&args, &ephemeral).await,
        Ok(None) => {}
    }
    let (store, _) = match open_store() {
        Ok(opened) => opened,
        Err(error) => return CommandOutcome::Failure(*error),
    };
    let entries: Vec<OrgIndexEntry> = store.list_orgs().into_iter().cloned().collect();
    let entry = match select_entry(&entries, args.target_org.as_deref()) {
        Ok(entry) => entry,
        Err(error) => return CommandOutcome::Failure(*error),
    };
    let credential = match store.get_credential(&entry.org_id, &entry.alias) {
        Ok(credential) => credential,
        Err(AuthError::CredentialStore(CredStoreError::NotFound(_))) => {
            return CommandOutcome::Failure(
                CommandError::operational(
                    "CREDENTIAL_MISSING",
                    format!("No stored credential for '{}'", entry.alias),
                )
                .with_context("Log in again to restore access to this org".to_string()),
            );
        }
        Err(error) => return CommandOutcome::Failure(storage_error(error)),
    };
    let instance_url = if credential.instance_url.is_empty() {
        entry.instance_url.clone()
    } else {
        credential.instance_url.clone()
    };

    let mut warnings = Vec::new();
    let connection = match usable_token(&store, &credential, &instance_url).await {
        Ok(token) => check_connection(&instance_url, &token).await,
        Err(reason) => {
            warnings.push(format!("Could not refresh the session: {reason}"));
            Connection::Unavailable("refresh failed".to_string())
        }
    };
    let (status, discovered_version) = match &connection {
        Connection::Connected { latest_version } => ("Connected", latest_version.clone()),
        Connection::Unavailable(reason) => {
            if !warnings.iter().any(|w| w.contains("refresh")) {
                warnings.push(format!("Could not verify the connection: {reason}"));
            }
            ("Unknown", None)
        }
    };
    let api_version = args.api_version.clone().or(discovered_version);
    let mut doc = build_display(&entry.org_id, &instance_url, api_version.as_deref(), status);
    add_fields(
        &mut doc,
        &[
            ("username", json!(entry.username)),
            ("alias", json!(entry.alias)),
            ("isDefaultUsername", json!(entry.is_default)),
        ],
    );
    CommandOutcome::Success {
        result: Some(doc),
        warnings,
    }
}

/// Display the org named by ephemeral token input. The target must be that
/// token's org ID so a headless token can never be shown as a different org.
async fn display_ephemeral(
    args: &OrgDisplayArgs,
    ephemeral: &EphemeralCredential,
) -> CommandOutcome {
    if let Some(target) = args
        .target_org
        .as_deref()
        .filter(|t| !t.eq_ignore_ascii_case(&ephemeral.org_id))
    {
        return CommandOutcome::Failure(
            CommandError::operational(
                "ORG_MISMATCH",
                format!(
                    "--target-org '{target}' does not match the org ID of the supplied access token"
                ),
            )
            .with_context(
                "With an ephemeral token, pass the token's org ID as the target".to_string(),
            ),
        );
    }
    let mut warnings = Vec::new();
    let (status, discovered) =
        match check_connection(&ephemeral.instance_url, ephemeral.token().expose()).await {
            Connection::Connected { latest_version } => ("Connected", latest_version),
            Connection::Unavailable(reason) => {
                warnings.push(format!("Could not verify the connection: {reason}"));
                ("Unknown", None)
            }
        };
    let api_version = args.api_version.clone().or(discovered);
    CommandOutcome::Success {
        result: Some(build_display(
            &ephemeral.org_id,
            &ephemeral.instance_url,
            api_version.as_deref(),
            status,
        )),
        warnings,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn display_redacts_token_and_omits_absent_version() {
        let doc = build_display("00Dabc", "https://a.my.salesforce.com", None, "Unknown");
        assert_eq!(doc["accessToken"], REDACTED);
        assert_eq!(doc["id"], "00Dabc");
        assert!(doc.get("apiVersion").is_none());
        let with_version = build_display(
            "00Dabc",
            "https://a.my.salesforce.com",
            Some("61.0"),
            "Connected",
        );
        assert_eq!(with_version["apiVersion"], "61.0");
    }

    #[test]
    fn latest_version_compares_numerically() {
        let entries = ["9.0", "58.0", "61.0", "100.0"]
            .map(|v| VersionEntry {
                version: v.to_string(),
            })
            .into();
        assert_eq!(latest_version(entries).as_deref(), Some("100.0"));
        assert_eq!(latest_version(Vec::new()), None);
    }

    #[test]
    fn api_version_validation() {
        assert!(valid_api_version("61.0"));
        assert!(valid_api_version("61"));
        for bad in ["", "v61", "61.", "61.0.1", "6a.0", "../x"] {
            assert!(!valid_api_version(bad), "{bad}");
        }
    }
}
