use thiserror::Error;

/// Safe transport error categories that never retain raw HTTP errors or bodies.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApiError {
    /// The configured instance URL is not a valid HTTPS Salesforce origin.
    #[error("invalid Salesforce instance origin")]
    InvalidOrigin,
    /// A request URL does not belong to the configured instance origin.
    #[error("request URL does not match the Salesforce instance origin")]
    OriginMismatch,
    /// The request URL contains disallowed credentials or a fragment.
    #[error("invalid request URL")]
    InvalidRequestUrl,
    /// A caller-provided content type is not a valid HTTP header value.
    #[error("invalid request content type")]
    InvalidRequestHeader,
    /// The bearer token is empty or cannot safely be used in an HTTP header.
    #[error("invalid bearer token")]
    InvalidToken,
    /// Client settings are outside supported bounded ranges.
    #[error("invalid HTTP client configuration")]
    InvalidConfiguration,
    /// The configured proxy could not be parsed or used.
    #[error("invalid proxy configuration")]
    InvalidProxy,
    /// A configured CA certificate could not be parsed.
    #[error("invalid TLS certificate configuration")]
    InvalidTlsConfiguration,
    /// The request or response body exceeded its configured deadline.
    #[error("HTTP request timed out")]
    Timeout,
    /// The request failed without exposing the underlying transport error.
    #[error("HTTP request failed")]
    RequestFailed,
    /// The response body could not be read without exposing its contents.
    #[error("HTTP response body could not be read")]
    ResponseRead,
    /// The response body exceeded the configured size limit.
    #[error("HTTP response body exceeds the configured limit of {limit} bytes")]
    ResponseTooLarge { limit: usize },
    /// The server returned a redirect, which this client never follows.
    #[error("unexpected HTTP redirect")]
    UnexpectedRedirect,
    /// The response body is not valid JSON.
    #[error("HTTP response body is not valid JSON")]
    InvalidJson,
    /// The bounded request-concurrency semaphore was closed.
    #[error("HTTP request concurrency control is unavailable")]
    ConcurrencyUnavailable,
}
