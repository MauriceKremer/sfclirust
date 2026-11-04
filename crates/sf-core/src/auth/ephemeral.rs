//! Ephemeral (headless) access-token input.
//!
//! A caller supplies `SF_ACCESS_TOKEN` (or `SFDX_ACCESS_TOKEN`) as
//! `<org id>!<access token>` together with `SF_RUST_INSTANCE_URL`. The token
//! lives only in this process: it is never written to the credential store or
//! the org index, and it cannot be reused by a later process.

use crate::auth::Secret;
use thiserror::Error;

const TOKEN_VARS: [&str; 2] = ["SF_ACCESS_TOKEN", "SFDX_ACCESS_TOKEN"];
const INSTANCE_URL_VAR: &str = "SF_RUST_INSTANCE_URL";

/// Why ephemeral input was rejected. Messages never contain the token.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EphemeralError {
    /// The token did not match `<org id>!<access token>`.
    #[error("The access token must have the form '<org id>!<access token>'")]
    MalformedToken,
    /// The instance URL is missing.
    #[error("{INSTANCE_URL_VAR} must be set to the org's https instance URL")]
    MissingInstanceUrl,
    /// The instance URL is not an https Salesforce origin.
    #[error("{INSTANCE_URL_VAR} must be an https Salesforce instance URL")]
    InvalidInstanceUrl,
}

/// An in-memory credential for one org.
pub struct EphemeralCredential {
    /// Salesforce organization ID from the token prefix.
    pub org_id: String,
    /// Validated HTTPS instance URL.
    pub instance_url: String,
    token: Secret,
}

impl EphemeralCredential {
    /// The access token.
    pub fn token(&self) -> &Secret {
        &self.token
    }

    /// Read ephemeral input through `lookup` (normally the process environment).
    ///
    /// Returns `Ok(None)` when no token variable is set, so callers fall back to
    /// persisted logins. Any set-but-invalid input is an error, never a fallback.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, EphemeralError> {
        let Some(raw) = TOKEN_VARS
            .iter()
            .find_map(|name| lookup(name).filter(|v| !v.is_empty()))
        else {
            return Ok(None);
        };
        let (org_id, token) = raw.split_once('!').ok_or(EphemeralError::MalformedToken)?;
        let valid_org =
            matches!(org_id.len(), 15 | 18) && org_id.chars().all(char::is_alphanumeric);
        if !valid_org || token.is_empty() || token.chars().any(char::is_whitespace) {
            return Err(EphemeralError::MalformedToken);
        }
        let instance_url = lookup(INSTANCE_URL_VAR)
            .filter(|v| !v.is_empty())
            .ok_or(EphemeralError::MissingInstanceUrl)?;
        let parsed =
            reqwest::Url::parse(&instance_url).map_err(|_| EphemeralError::InvalidInstanceUrl)?;
        let host = parsed.host_str().unwrap_or_default();
        let salesforce_host = host.ends_with(".salesforce.com")
            || host.ends_with(".force.com")
            || host.ends_with(".salesforce.mil")
            || host == "salesforce.com";
        if parsed.scheme() != "https"
            || !salesforce_host
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.port().is_some_and(|port| port != 443)
        {
            return Err(EphemeralError::InvalidInstanceUrl);
        }
        Ok(Some(Self {
            org_id: org_id.to_string(),
            instance_url: parsed.to_string(),
            token: Secret::new(token),
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    const ORG: &str = "00D000000000001AAA";

    #[test]
    fn absent_input_is_none_so_callers_use_persisted_logins() {
        assert!(
            EphemeralCredential::from_lookup(lookup(&[]))
                .unwrap()
                .is_none()
        );
        let empty = lookup(&[("SF_ACCESS_TOKEN", "")]);
        assert!(EphemeralCredential::from_lookup(empty).unwrap().is_none());
    }

    #[test]
    fn valid_input_yields_org_id_instance_and_secret_token() {
        let token = format!("{ORG}!tok123");
        let cred = EphemeralCredential::from_lookup(lookup(&[
            ("SF_ACCESS_TOKEN", &token),
            ("SF_RUST_INSTANCE_URL", "https://a.my.salesforce.com"),
        ]))
        .unwrap()
        .expect("credential");
        assert_eq!(cred.org_id, ORG);
        assert_eq!(cred.instance_url, "https://a.my.salesforce.com/");
        assert_eq!(cred.token().expose(), "tok123");
        assert!(!format!("{:?}", cred.token()).contains("tok123"));
    }

    #[test]
    fn sfdx_variable_is_a_fallback_name() {
        let token = format!("{ORG}!tok");
        let cred = EphemeralCredential::from_lookup(lookup(&[
            ("SFDX_ACCESS_TOKEN", &token),
            ("SF_RUST_INSTANCE_URL", "https://a.my.salesforce.com"),
        ]))
        .unwrap();
        assert!(cred.is_some());
    }

    #[test]
    fn malformed_tokens_never_fall_back_and_never_echo_the_secret() {
        for bad in [
            "nobang",
            "short!tok",
            &format!("{ORG}!"),
            &format!("{ORG}!a b"),
        ] {
            let error = EphemeralCredential::from_lookup(lookup(&[
                ("SF_ACCESS_TOKEN", bad),
                ("SF_RUST_INSTANCE_URL", "https://a.my.salesforce.com"),
            ]))
            .err()
            .expect("rejected");
            assert_eq!(error, EphemeralError::MalformedToken, "{bad}");
            assert!(!error.to_string().contains("nobang"));
        }
    }

    #[test]
    fn instance_url_is_required_and_must_be_https_salesforce() {
        let token = format!("{ORG}!tok");
        let missing = EphemeralCredential::from_lookup(lookup(&[("SF_ACCESS_TOKEN", &token)]));
        assert_eq!(missing.err(), Some(EphemeralError::MissingInstanceUrl));
        for url in [
            "http://a.my.salesforce.com",
            "https://evil.example.com",
            "https://a.my.salesforce.com/path",
            "https://user@a.my.salesforce.com",
            "not a url",
        ] {
            let result = EphemeralCredential::from_lookup(lookup(&[
                ("SF_ACCESS_TOKEN", &token),
                ("SF_RUST_INSTANCE_URL", url),
            ]));
            assert_eq!(
                result.err(),
                Some(EphemeralError::InvalidInstanceUrl),
                "{url}"
            );
        }
    }
}
