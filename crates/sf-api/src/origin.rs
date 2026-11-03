#[cfg(test)]
use std::net::IpAddr;

use crate::ApiError;

/// A validated Salesforce instance origin to which bearer credentials may be sent.
#[derive(Clone, Debug)]
pub struct InstanceOrigin {
    url: reqwest::Url,
    loopback_test: bool,
}

impl InstanceOrigin {
    /// Parses an HTTPS Salesforce origin with no path, query, credentials, or fragment.
    pub fn parse(value: &str) -> Result<Self, ApiError> {
        let url = reqwest::Url::parse(value).map_err(|_| ApiError::InvalidOrigin)?;
        let host = url.host_str().ok_or(ApiError::InvalidOrigin)?;

        if url.scheme() != "https"
            || !is_salesforce_host(host)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port().is_some_and(|port| port != 443)
        {
            return Err(ApiError::InvalidOrigin);
        }

        Ok(Self {
            url,
            loopback_test: false,
        })
    }

    /// Returns the validated base URL.
    pub fn base_url(&self) -> &reqwest::Url {
        &self.url
    }

    /// Resolves a relative or absolute request URL and verifies its origin.
    pub fn request_url(&self, value: &str) -> Result<reqwest::Url, ApiError> {
        let url = self
            .url
            .join(value)
            .map_err(|_| ApiError::InvalidRequestUrl)?;
        self.validate_request_url(&url)?;
        Ok(url)
    }

    /// Verifies that a request target stays on this instance and has no user info or fragment.
    pub fn validate_request_url(&self, url: &reqwest::Url) -> Result<(), ApiError> {
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(ApiError::InvalidRequestUrl);
        }
        if self.url.origin() != url.origin() {
            return Err(ApiError::OriginMismatch);
        }
        if !self.loopback_test && url.scheme() != "https" {
            return Err(ApiError::OriginMismatch);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn loopback_test(value: &str) -> Result<Self, ApiError> {
        let url = reqwest::Url::parse(value).map_err(|_| ApiError::InvalidOrigin)?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });

        if url.scheme() != "http"
            || !loopback
            || url.port().is_none()
            || url.path() != "/"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ApiError::InvalidOrigin);
        }

        Ok(Self {
            url,
            loopback_test: true,
        })
    }
}

fn is_salesforce_host(host: &str) -> bool {
    host == "salesforce.com"
        || host.ends_with(".salesforce.com")
        || host == "salesforce.mil"
        || host.ends_with(".salesforce.mil")
        || host.ends_with(".force.com")
}
