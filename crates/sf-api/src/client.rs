use std::{sync::Arc, time::Duration};

use reqwest::{
    Certificate, Client, Method, StatusCode, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;

use crate::{ApiError, InstanceOrigin};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 8;
const MAX_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CONCURRENT_REQUESTS: usize = 64;
const MAX_BEARER_TOKEN_BYTES: usize = 16 * 1024;
const MAX_ROOT_CERTIFICATES: usize = 16;

/// A bearer credential whose debug output is always redacted.
pub struct BearerToken<'a>(&'a str);

impl<'a> BearerToken<'a> {
    /// Validates a token for use in an Authorization header.
    pub fn new(value: &'a str) -> Result<Self, ApiError> {
        if value.is_empty()
            || value.len() > MAX_BEARER_TOKEN_BYTES
            || value.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            return Err(ApiError::InvalidToken);
        }
        HeaderValue::from_str(&format!("Bearer {value}")).map_err(|_| ApiError::InvalidToken)?;
        Ok(Self(value))
    }
}

impl std::fmt::Debug for BearerToken<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BearerToken(***)")
    }
}

/// Bounded settings for constructing a reusable HTTP client.
pub struct TransportSettings {
    connect_timeout: Duration,
    request_timeout: Duration,
    max_response_bytes: usize,
    max_concurrent_requests: usize,
    use_system_proxy: bool,
    proxy: Option<String>,
    root_certificates_pem: Vec<Vec<u8>>,
}

impl Default for TransportSettings {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_concurrent_requests: DEFAULT_MAX_CONCURRENT_REQUESTS,
            use_system_proxy: true,
            proxy: None,
            root_certificates_pem: Vec::new(),
        }
    }
}

impl TransportSettings {
    /// Sets the connect timeout; values are validated when building the client.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the complete request and response-body deadline.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the maximum response-body size.
    pub fn with_max_response_bytes(mut self, max_bytes: usize) -> Self {
        self.max_response_bytes = max_bytes;
        self
    }

    /// Sets the maximum number of requests in flight.
    pub fn with_max_concurrent_requests(mut self, max_requests: usize) -> Self {
        self.max_concurrent_requests = max_requests;
        self
    }

    /// Disables ambient system proxy configuration.
    pub fn without_system_proxy(mut self) -> Self {
        self.use_system_proxy = false;
        self
    }

    /// Configures an explicit proxy and disables ambient proxy settings.
    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    /// Adds a PEM-encoded CA certificate to the platform's verified root set.
    pub fn add_root_certificate_pem(mut self, certificate: impl Into<Vec<u8>>) -> Self {
        self.root_certificates_pem.push(certificate.into());
        self
    }
}

/// A reusable, bounded HTTP transport with origin-bound bearer authentication.
pub struct HttpClient {
    client: Client,
    permits: Arc<Semaphore>,
    max_response_bytes: usize,
    request_timeout: Duration,
}

impl Clone for HttpClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            permits: Arc::clone(&self.permits),
            max_response_bytes: self.max_response_bytes,
            request_timeout: self.request_timeout,
        }
    }
}

impl HttpClient {
    /// Builds a client using verified Rustls roots, explicit timeouts, and no redirects.
    pub fn new(settings: TransportSettings) -> Result<Self, ApiError> {
        if settings.connect_timeout.is_zero()
            || settings.request_timeout.is_zero()
            || settings.connect_timeout > MAX_TIMEOUT
            || settings.request_timeout > MAX_TIMEOUT
            || settings.max_response_bytes == 0
            || settings.max_response_bytes > MAX_RESPONSE_BYTES
            || settings.max_concurrent_requests == 0
            || settings.max_concurrent_requests > MAX_CONCURRENT_REQUESTS
            || settings.root_certificates_pem.len() > MAX_ROOT_CERTIFICATES
        {
            return Err(ApiError::InvalidConfiguration);
        }

        let mut builder = Client::builder()
            .connect_timeout(settings.connect_timeout)
            .timeout(settings.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(settings.max_concurrent_requests);

        if let Some(proxy) = settings.proxy {
            builder = builder.no_proxy();
            let proxy = reqwest::Proxy::all(&proxy).map_err(|_| ApiError::InvalidProxy)?;
            builder = builder.proxy(proxy);
        } else if !settings.use_system_proxy {
            builder = builder.no_proxy();
        }

        let has_custom_roots = !settings.root_certificates_pem.is_empty();
        let mut root_certificate_count = 0;
        for certificate in settings.root_certificates_pem {
            if certificate.len() > 1024 * 1024 {
                return Err(ApiError::InvalidConfiguration);
            }
            let certificates = Certificate::from_pem_bundle(&certificate)
                .map_err(|_| ApiError::InvalidTlsConfiguration)?;
            if certificates.is_empty() {
                return Err(ApiError::InvalidTlsConfiguration);
            }
            root_certificate_count += certificates.len();
            if root_certificate_count > MAX_ROOT_CERTIFICATES {
                return Err(ApiError::InvalidConfiguration);
            }
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }

        let client = builder.build().map_err(|_| {
            if has_custom_roots {
                ApiError::InvalidTlsConfiguration
            } else {
                ApiError::InvalidConfiguration
            }
        })?;
        Ok(Self {
            client,
            permits: Arc::new(Semaphore::new(settings.max_concurrent_requests)),
            max_response_bytes: settings.max_response_bytes,
            request_timeout: settings.request_timeout,
        })
    }

    /// Sends one authenticated request without following redirects.
    ///
    /// A body is sent with its caller-supplied content type. Non-2xx responses
    /// are returned to the caller for workflow-specific interpretation.
    pub async fn execute(
        &self,
        origin: &InstanceOrigin,
        token: &BearerToken<'_>,
        method: Method,
        url: &Url,
        body: Option<(&str, &[u8])>,
    ) -> Result<ApiResponse, ApiError> {
        tokio::time::timeout(
            self.request_timeout,
            self.execute_inner(origin, token, method, url, body),
        )
        .await
        .map_err(|_| ApiError::Timeout)?
    }

    async fn execute_inner(
        &self,
        origin: &InstanceOrigin,
        token: &BearerToken<'_>,
        method: Method,
        url: &Url,
        body: Option<(&str, &[u8])>,
    ) -> Result<ApiResponse, ApiError> {
        origin.validate_request_url(url)?;
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| ApiError::ConcurrencyUnavailable)?;

        let authorization = HeaderValue::from_str(&format!("Bearer {}", token.0))
            .map_err(|_| ApiError::InvalidToken)?;
        let mut request = self
            .client
            .request(method, url.clone())
            .header(AUTHORIZATION, authorization);
        if let Some((content_type, body)) = body {
            let content_type =
                HeaderValue::from_str(content_type).map_err(|_| ApiError::InvalidRequestHeader)?;
            request = request
                .header(CONTENT_TYPE, content_type)
                .body(body.to_vec());
        }

        let mut response = request.send().await.map_err(map_request_error)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(ApiError::UnexpectedRedirect);
        }
        let request_id = safe_request_id(response.headers());
        let retry_after = safe_retry_after(response.headers());
        let api_limit_info = safe_api_limit_info(response.headers());
        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_bytes as u64)
        {
            return Err(ApiError::ResponseTooLarge {
                limit: self.max_response_bytes,
            });
        }

        let mut response_body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_response_error)? {
            if response_body.len().saturating_add(chunk.len()) > self.max_response_bytes {
                return Err(ApiError::ResponseTooLarge {
                    limit: self.max_response_bytes,
                });
            }
            response_body.extend_from_slice(&chunk);
        }

        Ok(ApiResponse {
            status,
            body: response_body,
            request_id,
            retry_after,
            api_limit_info,
        })
    }
}

fn safe_request_id(headers: &HeaderMap) -> Option<String> {
    ["x-sfdc-request-id", "sforce-request-id", "x-request-id"]
        .iter()
        .filter_map(|name| headers.get(*name))
        .filter_map(|value| value.to_str().ok())
        .find(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .map(str::to_owned)
}

fn safe_retry_after(headers: &HeaderMap) -> Option<RetryAfter> {
    let value = headers.get("retry-after")?.to_str().ok()?;
    if value.len() > 128 {
        return None;
    }
    if let Ok(seconds) = value.parse::<u64>() {
        return (seconds <= 24 * 60 * 60).then(|| RetryAfter::Delay(Duration::from_secs(seconds)));
    }
    httpdate::parse_http_date(value).ok().map(RetryAfter::At)
}

fn safe_api_limit_info(headers: &HeaderMap) -> Option<ApiLimitInfo> {
    let value = headers.get("sforce-limit-info")?.to_str().ok()?;
    if value.len() > 256 {
        return None;
    }

    let mut api_usage = None;
    let mut per_app_api_usage = None;
    for item in value.split([',', ';']) {
        let Some((name, value)) = item.trim().split_once('=') else {
            continue;
        };
        match name.trim() {
            "api-usage" => {
                if api_usage.is_some() {
                    return None;
                }
                api_usage = Some(parse_api_limit_usage(value.trim())?);
            }
            "per-app-api-usage" => {
                if per_app_api_usage.is_some() {
                    return None;
                }
                per_app_api_usage = Some(parse_api_limit_usage(value.trim())?);
            }
            _ => {}
        }
    }
    (api_usage.is_some() || per_app_api_usage.is_some()).then_some(ApiLimitInfo {
        api_usage,
        per_app_api_usage,
    })
}

fn parse_api_limit_usage(value: &str) -> Option<ApiLimitUsage> {
    let (used, limit) = value.split_once('/')?;
    if used.is_empty()
        || limit.is_empty()
        || !used.bytes().all(|byte| byte.is_ascii_digit())
        || !limit.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let used = used.parse::<u64>().ok()?;
    let limit = limit.parse::<u64>().ok()?;
    (limit != 0).then_some(ApiLimitUsage { used, limit })
}

fn map_request_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::RequestFailed
    }
}

fn map_response_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::ResponseRead
    }
}

/// A fully buffered response whose size was checked against the client's limit.
pub struct ApiResponse {
    status: StatusCode,
    body: Vec<u8>,
    request_id: Option<String>,
    retry_after: Option<RetryAfter>,
    api_limit_info: Option<ApiLimitInfo>,
}

/// Parsed `Retry-After` guidance from an HTTP response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryAfter {
    /// Retry after this many seconds.
    Delay(Duration),
    /// Retry at the server-provided HTTP date.
    At(std::time::SystemTime),
}

/// API usage reported by Salesforce's `Sforce-Limit-Info` response header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiLimitInfo {
    api_usage: Option<ApiLimitUsage>,
    per_app_api_usage: Option<ApiLimitUsage>,
}

impl ApiLimitInfo {
    /// Returns org-wide API usage, if reported.
    pub fn api_usage(self) -> Option<ApiLimitUsage> {
        self.api_usage
    }

    /// Returns per-application API usage, if reported.
    pub fn per_app_api_usage(self) -> Option<ApiLimitUsage> {
        self.per_app_api_usage
    }

    /// Returns whether either reported quota is exhausted.
    pub fn is_exhausted(self) -> bool {
        self.api_usage.is_some_and(ApiLimitUsage::is_exhausted)
            || self
                .per_app_api_usage
                .is_some_and(ApiLimitUsage::is_exhausted)
    }
}

/// Used and total API request counts from a Salesforce limit header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiLimitUsage {
    used: u64,
    limit: u64,
}

impl ApiLimitUsage {
    /// Returns the number of API requests used in the reported period.
    pub fn used(self) -> u64 {
        self.used
    }

    /// Returns the total API request limit for the reported period.
    pub fn limit(self) -> u64 {
        self.limit
    }

    /// Returns whether Salesforce reports that the quota is exhausted.
    pub fn is_exhausted(self) -> bool {
        self.used >= self.limit
    }
}

impl ApiResponse {
    /// Returns the HTTP status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns a validated request identifier from known Salesforce headers.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Returns a parsed delta-seconds or HTTP-date `Retry-After` value.
    pub fn retry_after(&self) -> Option<RetryAfter> {
        self.retry_after
    }

    /// Returns parsed API quota usage, if Salesforce provided it.
    pub fn api_limit_info(&self) -> Option<ApiLimitInfo> {
        self.api_limit_info
    }

    /// Returns the bounded response body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Consumes the response and returns its bounded body.
    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    /// Deserializes the response body without exposing parser details or body data.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ApiError> {
        serde_json::from_slice(&self.body).map_err(|_| ApiError::InvalidJson)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use std::{sync::Arc, time::Duration};

    use reqwest::Method;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string, header, method, path},
    };

    use crate::{ApiError, BearerToken, HttpClient, InstanceOrigin, TransportSettings};

    fn settings() -> TransportSettings {
        TransportSettings::default().without_system_proxy()
    }

    async fn test_origin(server: &MockServer) -> InstanceOrigin {
        InstanceOrigin::loopback_test(&server.uri()).expect("wiremock loopback origin")
    }

    #[tokio::test]
    async fn sends_bearer_header_to_the_validated_origin() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/resource").expect("request URL");
        Mock::given(method("GET"))
            .and(path("/resource"))
            .and(header("authorization", "Bearer unit-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let response = client
            .execute(&origin, &token, Method::GET, &url, None)
            .await
            .expect("authenticated request");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.body(), b"ok");
    }

    #[tokio::test]
    async fn sends_caller_selected_content_type_and_body() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/soap").expect("request URL");
        Mock::given(method("POST"))
            .and(path("/soap"))
            .and(header("content-type", "text/xml; charset=utf-8"))
            .and(body_string("<Envelope/>"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        client
            .execute(
                &origin,
                &token,
                Method::POST,
                &url,
                Some(("text/xml; charset=utf-8", b"<Envelope/>")),
            )
            .await
            .expect("request with XML payload");
    }

    #[tokio::test]
    async fn parses_salesforce_api_limit_usage() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/limits").expect("request URL");
        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        Mock::given(method("GET"))
            .and(path("/limits"))
            .respond_with(ResponseTemplate::new(200).insert_header(
                "Sforce-Limit-Info",
                "api-usage=4999/5000, per-app-api-usage=2000/2000",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let response = client
            .execute(&origin, &token, Method::GET, &url, None)
            .await
            .expect("HTTP response");
        let limits = response.api_limit_info().expect("API usage metadata");
        let usage = limits.api_usage().expect("org API usage");

        assert_eq!(usage.used(), 4_999);
        assert_eq!(usage.limit(), 5_000);
        assert!(
            limits
                .per_app_api_usage()
                .expect("per-app API usage")
                .is_exhausted()
        );
        assert!(limits.is_exhausted());
    }

    #[tokio::test]
    async fn ignores_malformed_salesforce_api_limit_usage() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/limits").expect("request URL");
        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        Mock::given(method("GET"))
            .and(path("/limits"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Sforce-Limit-Info", "api-usage=5000/not-a-number"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let response = client
            .execute(&origin, &token, Method::GET, &url, None)
            .await
            .expect("HTTP response");

        assert_eq!(response.api_limit_info(), None);
    }

    #[tokio::test]
    async fn rejects_invalid_content_type_before_sending_a_request() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/invalid-header").expect("request URL");
        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");

        let error = client
            .execute(
                &origin,
                &token,
                Method::POST,
                &url,
                Some(("text/plain\r\nAuthorization: Bearer injected", b"payload")),
            )
            .await
            .err()
            .expect("invalid content type must be rejected");

        assert_eq!(error, ApiError::InvalidRequestHeader);
        assert!(
            server
                .received_requests()
                .await
                .expect("request count")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rejects_a_different_origin_before_sending_credentials() {
        let first = MockServer::start().await;
        let second = MockServer::start().await;
        let origin = test_origin(&first).await;
        let target = reqwest::Url::parse(&second.uri()).expect("second origin URL");
        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");

        let error = client
            .execute(&origin, &token, Method::GET, &target, None)
            .await
            .err()
            .expect("cross-origin URL must be rejected");

        assert_eq!(error, ApiError::OriginMismatch);
        assert_eq!(
            second
                .received_requests()
                .await
                .expect("request count")
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn does_not_follow_redirects_to_a_second_origin() {
        let first = MockServer::start().await;
        let second = MockServer::start().await;
        let origin = test_origin(&first).await;
        let target = second.uri();
        let url = origin.request_url("/redirect").expect("request URL");
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target))
            .mount(&first)
            .await;

        let client = HttpClient::new(settings()).expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let error = client
            .execute(&origin, &token, Method::GET, &url, None)
            .await
            .err()
            .expect("redirect must be rejected");

        assert_eq!(error, ApiError::UnexpectedRedirect);
        assert_eq!(
            second
                .received_requests()
                .await
                .expect("request count")
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn bounds_response_size_and_sanitizes_json_errors() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let too_large_url = origin.request_url("/large").expect("request URL");
        let malformed_url = origin.request_url("/malformed").expect("request URL");
        Mock::given(method("GET"))
            .and(path("/large"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("012345678901234567890123456789012"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/malformed"))
            .respond_with(ResponseTemplate::new(200).set_body_string("sensitive malformed body"))
            .mount(&server)
            .await;

        let client =
            HttpClient::new(settings().with_max_response_bytes(32)).expect("bounded HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let error = client
            .execute(&origin, &token, Method::GET, &too_large_url, None)
            .await
            .err()
            .expect("oversized response must fail");
        assert_eq!(error, ApiError::ResponseTooLarge { limit: 32 });

        let error = client
            .execute(&origin, &token, Method::GET, &malformed_url, None)
            .await
            .expect("malformed JSON body is transport-valid")
            .json::<serde_json::Value>()
            .expect_err("malformed JSON must fail");
        assert_eq!(error, ApiError::InvalidJson);
        assert!(!error.to_string().contains("sensitive malformed body"));
    }

    #[tokio::test]
    async fn request_deadline_covers_response_body() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/slow").expect("request URL");
        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("ok")
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;

        let client = HttpClient::new(settings().with_request_timeout(Duration::from_millis(25)))
            .expect("HTTP client");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let error = client
            .execute(&origin, &token, Method::GET, &url, None)
            .await
            .err()
            .expect("slow response must time out");

        assert_eq!(error, ApiError::Timeout);
    }

    #[tokio::test]
    async fn request_deadline_includes_concurrency_wait() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/queued").expect("request URL");
        let client = HttpClient::new(
            settings()
                .with_max_concurrent_requests(1)
                .with_request_timeout(Duration::from_millis(25)),
        )
        .expect("HTTP client");
        let held_permit = client
            .permits
            .acquire()
            .await
            .expect("test holds concurrency permit");
        let token = BearerToken::new("unit-test-token").expect("test token");

        let error = client
            .execute(&origin, &token, Method::GET, &url, None)
            .await
            .err()
            .expect("queued request must time out");

        assert_eq!(error, ApiError::Timeout);
        assert!(
            server
                .received_requests()
                .await
                .expect("request count")
                .is_empty()
        );
        drop(held_permit);
    }

    #[tokio::test]
    async fn bounds_concurrent_requests() {
        let server = MockServer::start().await;
        let origin = test_origin(&server).await;
        let url = origin.request_url("/slow").expect("request URL");
        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("ok")
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;

        let client = Arc::new(
            HttpClient::new(settings().with_max_concurrent_requests(1))
                .expect("single-concurrency client"),
        );
        let first_client = Arc::clone(&client);
        let first_origin = origin.clone();
        let first_url = url.clone();
        let first = tokio::spawn(async move {
            let token = BearerToken::new("unit-test-token").expect("test token");
            first_client
                .execute(&first_origin, &token, Method::GET, &first_url, None)
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            while client.permits.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first request acquired the only permit");
        let second_permit = tokio::time::timeout(
            Duration::from_millis(20),
            Arc::clone(&client.permits).acquire_owned(),
        )
        .await;

        assert!(second_permit.is_err());
        first
            .await
            .expect("request task")
            .expect("request succeeds");
    }
}
