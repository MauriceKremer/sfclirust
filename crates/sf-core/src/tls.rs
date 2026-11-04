//! Opt-in extra CA certificates for networks that inspect TLS.
//!
//! Verification is never disabled. A corporate proxy that re-signs HTTPS traffic
//! is trusted only when the user points at its CA bundle. `SF_RUST_CA_BUNDLE`
//! wins; `NODE_EXTRA_CA_CERTS` is honored too, as the upstream CLI does.

use std::path::Path;

/// Preferred variable naming a PEM file of extra trusted CA certificates.
pub const CA_BUNDLE_ENV: &str = "SF_RUST_CA_BUNDLE";
/// Variable the upstream CLI honors for the same purpose.
pub const NODE_CA_ENV: &str = "NODE_EXTRA_CA_CERTS";
const MAX_BUNDLE_BYTES: u64 = 1024 * 1024;

/// A configured CA bundle that could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TlsConfigError {
    /// The named bundle file is unreadable, too large, or not PEM certificates.
    #[error("CA bundle from {variable} is unusable: {reason}")]
    UnusableBundle {
        /// Environment variable that named the bundle.
        variable: &'static str,
        /// Why the bundle was rejected; never includes certificate contents.
        reason: &'static str,
    },
}

/// PEM bytes of the configured extra CA bundle, or `None` when none is set.
///
/// A set but unusable bundle is an error: silently ignoring it would hide why
/// every HTTPS request then fails certificate verification.
pub fn extra_ca_bundle(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<Vec<u8>>, TlsConfigError> {
    for variable in [CA_BUNDLE_ENV, NODE_CA_ENV] {
        let Some(path) = lookup(variable).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        return read_bundle(variable, Path::new(path.trim())).map(Some);
    }
    Ok(None)
}

fn read_bundle(variable: &'static str, path: &Path) -> Result<Vec<u8>, TlsConfigError> {
    let unusable = |reason| TlsConfigError::UnusableBundle { variable, reason };
    let size = std::fs::metadata(path)
        .map_err(|_| unusable("file cannot be read"))?
        .len();
    if size > MAX_BUNDLE_BYTES {
        return Err(unusable("file is larger than 1 MiB"));
    }
    let bytes = std::fs::read(path).map_err(|_| unusable("file cannot be read"))?;
    if !bytes
        .windows(b"-----BEGIN CERTIFICATE-----".len())
        .any(|w| w == b"-----BEGIN CERTIFICATE-----")
    {
        return Err(unusable("no PEM certificates found"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use std::io::Write;

    fn bundle(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(contents.as_bytes()).expect("write");
        file
    }

    const PEM: &str = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";

    #[test]
    fn nothing_configured_means_no_extra_roots() {
        assert_eq!(extra_ca_bundle(|_| None).unwrap(), None);
        assert_eq!(extra_ca_bundle(|_| Some("  ".into())).unwrap(), None);
    }

    #[test]
    fn reads_the_bundle_named_by_the_preferred_variable() {
        let file = bundle(PEM);
        let path = file.path().to_string_lossy().to_string();
        let got = extra_ca_bundle(|name| (name == CA_BUNDLE_ENV).then(|| path.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(got, PEM.as_bytes());
    }

    #[test]
    fn falls_back_to_the_upstream_variable_and_prefers_ours() {
        let node = bundle(PEM);
        let node_path = node.path().to_string_lossy().to_string();
        let got = extra_ca_bundle(|name| (name == NODE_CA_ENV).then(|| node_path.clone())).unwrap();
        assert!(got.is_some());

        let ours = bundle(&format!("{PEM}# ours\n"));
        let ours_path = ours.path().to_string_lossy().to_string();
        let got = extra_ca_bundle(|name| match name {
            CA_BUNDLE_ENV => Some(ours_path.clone()),
            NODE_CA_ENV => Some(node_path.clone()),
            _ => None,
        })
        .unwrap()
        .unwrap();
        assert!(String::from_utf8(got).unwrap().contains("# ours"));
    }

    #[test]
    fn unusable_bundles_are_errors_that_do_not_echo_contents() {
        let missing = extra_ca_bundle(|_| Some("/no/such/file.pem".into()));
        assert!(matches!(
            missing,
            Err(TlsConfigError::UnusableBundle { .. })
        ));

        let junk = bundle("secret-looking text");
        let path = junk.path().to_string_lossy().to_string();
        let error = extra_ca_bundle(|_| Some(path.clone())).unwrap_err();
        assert!(!error.to_string().contains("secret-looking"));
    }
}
