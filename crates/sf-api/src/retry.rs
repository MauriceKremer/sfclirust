use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use thiserror::Error;
use tokio::{time::Instant, time::sleep_until};
use tokio_util::sync::CancellationToken;

use crate::{
    ApiError, ApiResponse, BearerToken, HttpClient, InstanceOrigin, Method, RetryAfter, Url,
    rest::{SalesforceApiError, SalesforceErrorCategory},
};

const MAX_ATTEMPTS: usize = 5;
const MAX_ELAPSED: Duration = Duration::from_secs(600);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
type SleepFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type Sleeper = std::sync::Arc<dyn Fn(Duration) -> SleepFuture + Send + Sync>;

/// Whether replaying an operation is safe under the operation's own semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationRetryability {
    /// The operation is safe to repeat after transient transport or server failures.
    Safe,
    /// The operation may be repeated only after HTTP 429 or
    /// `REQUEST_LIMIT_EXCEEDED`; transport failures and other server errors are
    /// not retried.
    Conditional,
    /// The operation must never be repeated automatically.
    NonRetryable,
}

/// A validated identifier callers can use to recover an uncertain operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationId(String);

impl OperationId {
    /// Validates a bounded identifier safe to include in diagnostics.
    pub fn new(value: impl Into<String>) -> Result<Self, RetryError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(RetryError::InvalidOperationId);
        }
        Ok(Self(value))
    }

    /// Returns the validated operation identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Caller-supplied retry semantics and optional recovery identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryOperation {
    retryability: OperationRetryability,
    operation_id: Option<OperationId>,
}

impl RetryOperation {
    /// Declares the operation's replay semantics; HTTP method alone is not used.
    pub fn new(retryability: OperationRetryability) -> Self {
        Self {
            retryability,
            operation_id: None,
        }
    }

    /// Attaches an operation identifier for uncertain-outcome recovery.
    pub fn with_operation_id(mut self, operation_id: OperationId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }
}

/// A replayable HTTP request paired with its operation-level retry semantics.
pub struct RetryRequest<'request, 'token> {
    origin: &'request InstanceOrigin,
    token: &'request BearerToken<'token>,
    method: Method,
    url: &'request Url,
    body: Option<(&'request str, &'request [u8])>,
    operation: RetryOperation,
}

impl<'request, 'token> RetryRequest<'request, 'token> {
    /// Builds a request whose body, if any, can be added as stable bytes.
    pub fn new(
        origin: &'request InstanceOrigin,
        token: &'request BearerToken<'token>,
        method: Method,
        url: &'request Url,
        operation: RetryOperation,
    ) -> Self {
        Self {
            origin,
            token,
            method,
            url,
            body: None,
            operation,
        }
    }

    /// Sets a buffered body that will be identical on each permitted attempt.
    pub fn with_body(mut self, content_type: &'request str, body: &'request [u8]) -> Self {
        self.body = Some((content_type, body));
        self
    }
}

/// Bounds attempts, total elapsed time, and exponential backoff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: usize,
    max_elapsed: Duration,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl RetryPolicy {
    /// Builds a bounded policy. `max_attempts` includes the initial request and
    /// cannot exceed 10; elapsed time is capped at 10 minutes and backoff at 30
    /// seconds.
    pub fn new(
        max_attempts: usize,
        max_elapsed: Duration,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Result<Self, RetryError> {
        if max_attempts == 0
            || max_attempts > MAX_ATTEMPTS
            || max_elapsed.is_zero()
            || max_elapsed > MAX_ELAPSED
            || initial_backoff > max_backoff
            || max_backoff > MAX_BACKOFF
        {
            return Err(RetryError::InvalidPolicy);
        }
        Ok(Self {
            max_attempts,
            max_elapsed,
            initial_backoff,
            max_backoff,
        })
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            max_elapsed: Duration::from_secs(60),
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(10),
        }
    }
}

/// Executes authenticated requests under explicit operation retry semantics.
pub struct RetryClient {
    transport: HttpClient,
    policy: RetryPolicy,
    seed: u64,
    sequence: AtomicU64,
    sleeper: Sleeper,
}

impl RetryClient {
    /// Creates a retry executor with the supplied transport and bounded policy.
    pub fn new(transport: HttpClient, policy: RetryPolicy) -> Self {
        Self {
            transport,
            policy,
            seed: fastrand::u64(..),
            sequence: AtomicU64::new(0),
            sleeper: std::sync::Arc::new(|delay| Box::pin(tokio::time::sleep(delay))),
        }
    }

    #[cfg(test)]
    fn with_seed(transport: HttpClient, policy: RetryPolicy, seed: u64) -> Self {
        Self {
            transport,
            policy,
            seed,
            sequence: AtomicU64::new(0),
            sleeper: std::sync::Arc::new(|delay| Box::pin(tokio::time::sleep(delay))),
        }
    }

    #[cfg(test)]
    fn with_seed_and_sleeper(
        transport: HttpClient,
        policy: RetryPolicy,
        seed: u64,
        sleeper: Sleeper,
    ) -> Self {
        Self {
            transport,
            policy,
            seed,
            sequence: AtomicU64::new(0),
            sleeper,
        }
    }

    /// Sends a request and retries only when its explicit semantics permit replay.
    ///
    /// Non-success HTTP responses are returned if the policy stops retrying.
    /// Cancellation or deadline expiry during an in-flight non-safe operation
    /// returns an uncertain-outcome error with the supplied operation ID.
    pub async fn execute(
        &self,
        request: RetryRequest<'_, '_>,
        cancellation: CancellationToken,
    ) -> Result<ApiResponse, RetryError> {
        let RetryRequest {
            origin,
            token,
            method,
            url,
            body,
            operation,
        } = request;
        if cancellation.is_cancelled() {
            return Err(RetryError::Cancelled);
        }
        let deadline = Instant::now() + self.policy.max_elapsed;
        let seed = self
            .seed
            .wrapping_add(self.sequence.fetch_add(1, Ordering::Relaxed));
        let mut attempt = 1;

        loop {
            let result = tokio::select! {
                biased;
                result = self.transport.execute(origin, token, method.clone(), url, body) => result,
                _ = cancellation.cancelled() => {
                    return Err(aborted(&operation, AbortReason::Cancelled));
                }
                _ = sleep_until(deadline) => {
                    return Err(aborted(&operation, AbortReason::DeadlineExceeded));
                }
            };

            match result {
                Err(error)
                    if operation.retryability == OperationRetryability::Safe
                        && is_transient_transport_error(&error)
                        && attempt < self.policy.max_attempts =>
                {
                    let delay = self.backoff(seed, attempt);
                    if Instant::now() + delay >= deadline {
                        return Err(RetryError::DeadlineExceeded);
                    }
                    wait_before_retry(&cancellation, deadline, delay, &self.sleeper).await?;
                    attempt += 1;
                }
                Err(error) => {
                    if operation.retryability != OperationRetryability::Safe && is_ambiguous(&error)
                    {
                        return Err(RetryError::OutcomeUncertain {
                            operation_id: operation.operation_id,
                            cause: OutcomeUncertainty::Transport(error),
                        });
                    }
                    return Err(RetryError::Transport(error));
                }
                Ok(response) => {
                    if attempt < self.policy.max_attempts
                        && let Some(delay) =
                            self.retry_delay(operation.retryability, &response, seed, attempt)
                    {
                        if Instant::now() + delay >= deadline {
                            return Ok(response);
                        }
                        wait_before_retry(&cancellation, deadline, delay, &self.sleeper).await?;
                        attempt += 1;
                        continue;
                    }
                    return Ok(response);
                }
            }
        }
    }

    fn retry_delay(
        &self,
        retryability: OperationRetryability,
        response: &ApiResponse,
        seed: u64,
        attempt: usize,
    ) -> Option<Duration> {
        if response
            .api_limit_info()
            .is_some_and(|limit| limit.is_exhausted())
        {
            return None;
        }

        let is_rate_limited = response.status() == crate::StatusCode::TOO_MANY_REQUESTS
            || SalesforceApiError::from_response(
                response.status(),
                response.body(),
                response.request_id(),
                response.retry_after(),
            )
            .category()
                == SalesforceErrorCategory::RateLimited;
        let transient_status = response.status() == crate::StatusCode::REQUEST_TIMEOUT
            || matches!(response.status().as_u16(), 500 | 502 | 503 | 504);
        let retryable = match retryability {
            OperationRetryability::Safe => is_rate_limited || transient_status,
            OperationRetryability::Conditional => is_rate_limited,
            OperationRetryability::NonRetryable => false,
        };
        if !retryable {
            return None;
        }

        let backoff = self.backoff(seed, attempt);
        let server_delay = match response.retry_after() {
            Some(RetryAfter::Delay(delay)) => delay,
            Some(RetryAfter::At(retry_at)) => retry_at
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO),
            None => Duration::ZERO,
        };
        if server_delay > self.policy.max_backoff {
            return None;
        }
        Some(backoff.max(server_delay))
    }

    fn backoff(&self, seed: u64, attempt: usize) -> Duration {
        let exponent = 1_u32
            .checked_shl(attempt.saturating_sub(1) as u32)
            .unwrap_or(u32::MAX);
        let cap = self
            .policy
            .initial_backoff
            .saturating_mul(exponent)
            .min(self.policy.max_backoff);
        let cap_nanos = cap.as_nanos().min(u64::MAX as u128) as u64;
        let attempt_seed = seed.wrapping_add(attempt as u64);
        let nanos = fastrand::Rng::with_seed(attempt_seed).u64(0..=cap_nanos);
        Duration::from_nanos(nanos)
    }
}

async fn wait_before_retry(
    cancellation: &CancellationToken,
    deadline: Instant,
    delay: Duration,
    sleeper: &Sleeper,
) -> Result<(), RetryError> {
    let retry_at = Instant::now() + delay;
    if retry_at >= deadline {
        return Err(RetryError::DeadlineExceeded);
    }
    let sleep = sleeper(delay);
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(RetryError::Cancelled),
        _ = sleep_until(deadline) => Err(RetryError::DeadlineExceeded),
        _ = sleep => Ok(()),
    }
}

fn is_transient_transport_error(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Timeout | ApiError::RequestFailed | ApiError::ResponseRead
    )
}

fn is_ambiguous(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Timeout
            | ApiError::RequestFailed
            | ApiError::ResponseRead
            | ApiError::ResponseTooLarge { .. }
            | ApiError::UnexpectedRedirect
    )
}

enum AbortReason {
    Cancelled,
    DeadlineExceeded,
}

fn aborted(operation: &RetryOperation, reason: AbortReason) -> RetryError {
    if operation.retryability != OperationRetryability::Safe {
        let cause = match reason {
            AbortReason::Cancelled => OutcomeUncertainty::Cancelled,
            AbortReason::DeadlineExceeded => OutcomeUncertainty::DeadlineExceeded,
        };
        return RetryError::OutcomeUncertain {
            operation_id: operation.operation_id.clone(),
            cause,
        };
    }
    match reason {
        AbortReason::Cancelled => RetryError::Cancelled,
        AbortReason::DeadlineExceeded => RetryError::DeadlineExceeded,
    }
}

/// Why an in-flight operation's remote outcome could not be established.
#[derive(Debug, PartialEq, Eq)]
pub enum OutcomeUncertainty {
    /// The caller cancelled while the request was in flight.
    Cancelled,
    /// The total operation deadline expired while the request was in flight.
    DeadlineExceeded,
    /// The transport failed after the request may have reached Salesforce.
    Transport(ApiError),
}

/// A safe retry executor failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RetryError {
    /// Invalid attempt or timing bounds were supplied.
    #[error("invalid retry policy")]
    InvalidPolicy,
    /// An operation identifier contains unsupported or oversized data.
    #[error("invalid operation identifier")]
    InvalidOperationId,
    /// The operation was cancelled before a retry or request completed.
    #[error("Salesforce request was cancelled")]
    Cancelled,
    /// The total retry deadline elapsed.
    #[error("Salesforce retry deadline elapsed")]
    DeadlineExceeded,
    /// The transport failed and the operation could not safely be retried.
    #[error(transparent)]
    Transport(ApiError),
    /// A mutation may have been accepted before the transport stopped.
    #[error("Salesforce operation outcome is uncertain")]
    OutcomeUncertain {
        /// A known identifier to use for recovery, if available.
        operation_id: Option<OperationId>,
        /// The event that prevented a conclusive response.
        cause: OutcomeUncertainty,
    },
}

impl RetryError {
    /// Returns the known operation ID when the outcome is uncertain.
    pub fn operation_id(&self) -> Option<&OperationId> {
        match self {
            Self::OutcomeUncertain { operation_id, .. } => operation_id.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::Sleeper;
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use reqwest::{Method, StatusCode};
    use tokio_util::sync::CancellationToken;
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{body_string, method, path},
    };

    use crate::{
        ApiError, BearerToken, HttpClient, InstanceOrigin, RetryAfter, TransportSettings, Url,
        retry::{
            OperationId, OperationRetryability, OutcomeUncertainty, RetryClient, RetryError,
            RetryOperation, RetryPolicy, RetryRequest, wait_before_retry,
        },
    };

    fn retry_request<'request, 'token>(
        origin: &'request InstanceOrigin,
        token: &'request BearerToken<'token>,
        method: Method,
        url: &'request Url,
        operation: RetryOperation,
    ) -> RetryRequest<'request, 'token> {
        RetryRequest::new(origin, token, method, url, operation)
    }

    fn tokio_sleeper() -> Sleeper {
        Arc::new(|delay| Box::pin(tokio::time::sleep(delay)))
    }

    #[tokio::test]
    async fn retries_a_safe_request_only_up_to_the_configured_attempt_count() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/retry").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(3, Duration::from_secs(10), Duration::ZERO, Duration::ZERO)
            .expect("valid retry policy");
        let client = RetryClient::new(transport, policy);

        Mock::given(method("GET"))
            .and(path("/retry"))
            .respond_with(ResponseTemplate::new(StatusCode::SERVICE_UNAVAILABLE))
            .expect(3)
            .mount(&server)
            .await;

        let response = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::GET,
                    &target,
                    RetryOperation::new(OperationRetryability::Safe),
                ),
                CancellationToken::new(),
            )
            .await
            .expect("exhausted retries return the last HTTP response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn retryability_is_not_inferred_from_the_http_method() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/read").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(3, Duration::from_secs(10), Duration::ZERO, Duration::ZERO)
            .expect("valid retry policy");
        let client = RetryClient::new(transport, policy);

        Mock::given(method("GET"))
            .and(path("/read"))
            .respond_with(ResponseTemplate::new(StatusCode::SERVICE_UNAVAILABLE))
            .expect(1)
            .mount(&server)
            .await;

        let response = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::GET,
                    &target,
                    RetryOperation::new(OperationRetryability::NonRetryable),
                ),
                CancellationToken::new(),
            )
            .await
            .expect("non-retryable response is returned");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn conditional_mutations_are_not_retried_on_generic_server_errors() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/submit").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(3, Duration::from_secs(10), Duration::ZERO, Duration::ZERO)
            .expect("valid retry policy");
        let client = RetryClient::new(transport, policy);

        Mock::given(method("POST"))
            .and(path("/submit"))
            .respond_with(ResponseTemplate::new(StatusCode::SERVICE_UNAVAILABLE))
            .expect(1)
            .mount(&server)
            .await;

        let response = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::POST,
                    &target,
                    RetryOperation::new(OperationRetryability::Conditional),
                )
                .with_body("application/json", b"{}"),
                CancellationToken::new(),
            )
            .await
            .expect("conditional operation returns generic server errors");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn does_not_retry_before_a_server_delay_exceeds_policy_bounds() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/limited").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(
            3,
            Duration::from_secs(10),
            Duration::ZERO,
            Duration::from_secs(5),
        )
        .expect("valid retry policy");
        let client = RetryClient::new(transport, policy);

        Mock::given(method("POST"))
            .and(path("/limited"))
            .respond_with(
                ResponseTemplate::new(StatusCode::TOO_MANY_REQUESTS)
                    .insert_header("Retry-After", "3600"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let response = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::POST,
                    &target,
                    RetryOperation::new(OperationRetryability::Conditional),
                )
                .with_body("application/json", b"{}"),
                CancellationToken::new(),
            )
            .await
            .expect("response is returned instead of retrying early");

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.retry_after(),
            Some(RetryAfter::Delay(Duration::from_secs(3600)))
        );
    }

    #[tokio::test]
    async fn does_not_retry_when_the_reported_api_quota_is_exhausted() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/limited").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(
            3,
            Duration::from_secs(10),
            Duration::ZERO,
            Duration::from_secs(5),
        )
        .expect("valid retry policy");
        let client = RetryClient::new(transport, policy);

        Mock::given(method("POST"))
            .and(path("/limited"))
            .respond_with(
                ResponseTemplate::new(StatusCode::TOO_MANY_REQUESTS)
                    .insert_header("Retry-After", "1")
                    .insert_header(
                        "Sforce-Limit-Info",
                        "api-usage=4999/5000, per-app-api-usage=2000/2000",
                    )
                    .set_body_json(serde_json::json!([{
                        "errorCode": "REQUEST_LIMIT_EXCEEDED",
                        "message": "API quota exceeded"
                    }])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let response = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::POST,
                    &target,
                    RetryOperation::new(OperationRetryability::Conditional),
                )
                .with_body("application/json", b"{}"),
                CancellationToken::new(),
            )
            .await
            .expect("exhausted quota is returned without replay");

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(
            response
                .api_limit_info()
                .expect("quota metadata")
                .is_exhausted()
        );
    }

    #[tokio::test]
    async fn cancellation_of_an_in_flight_mutation_returns_its_operation_id() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/submit").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::default();
        let client = RetryClient::new(transport, policy);
        let cancellation = CancellationToken::new();
        let operation_id = OperationId::new("deploy-job-123").expect("operation ID");

        Mock::given(method("POST"))
            .and(path("/submit"))
            .respond_with(ResponseTemplate::new(StatusCode::OK).set_delay(Duration::from_secs(5)))
            .expect(1)
            .mount(&server)
            .await;

        let request_cancellation = cancellation.clone();
        let request = tokio::spawn(async move {
            client
                .execute(
                    retry_request(
                        &origin,
                        &token,
                        Method::POST,
                        &target,
                        RetryOperation::new(OperationRetryability::NonRetryable)
                            .with_operation_id(operation_id),
                    )
                    .with_body("application/json", b"{}"),
                    request_cancellation,
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if server
                    .received_requests()
                    .await
                    .is_some_and(|requests| !requests.is_empty())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("request reached the server");
        cancellation.cancel();

        let error = match request.await.expect("request task") {
            Ok(_) => panic!("cancelled mutation unexpectedly returned a response"),
            Err(error) => error,
        };
        assert!(matches!(
            &error,
            RetryError::OutcomeUncertain {
                cause: OutcomeUncertainty::Cancelled,
                ..
            }
        ));
        assert_eq!(
            error.operation_id().map(OperationId::as_str),
            Some("deploy-job-123")
        );
    }

    #[tokio::test]
    async fn a_mutation_deadline_returns_an_uncertain_outcome() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/submit").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(3, Duration::from_millis(25), Duration::ZERO, Duration::ZERO)
            .expect("valid retry policy");
        let client = RetryClient::new(transport, policy);

        Mock::given(method("POST"))
            .and(path("/submit"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_delay(Duration::from_millis(250)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::POST,
                    &target,
                    RetryOperation::new(OperationRetryability::NonRetryable)
                        .with_operation_id(OperationId::new("job-789").expect("operation ID")),
                )
                .with_body("application/json", b"{}"),
                CancellationToken::new(),
            )
            .await;
        let error = match result {
            Ok(_) => panic!("deadline-expired mutation unexpectedly returned a response"),
            Err(error) => error,
        };

        assert!(matches!(
            &error,
            RetryError::OutcomeUncertain {
                cause: OutcomeUncertainty::DeadlineExceeded,
                ..
            }
        ));
        assert_eq!(
            error.operation_id().map(OperationId::as_str),
            Some("job-789")
        );
    }

    #[test]
    fn seeded_backoff_is_reproducible_and_bounded() {
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(
            3,
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(5),
        )
        .expect("valid retry policy");
        let first = RetryClient::with_seed(transport.clone(), policy.clone(), 42);
        let second = RetryClient::with_seed(transport, policy, 42);

        let first_delay = first.backoff(first.seed, 2);
        let second_delay = second.backoff(second.seed, 2);

        assert_eq!(first_delay, second_delay);
        assert!(first_delay <= Duration::from_secs(2));
    }

    #[test]
    fn retry_policy_rejects_unbounded_attempts_and_delays() {
        assert_eq!(
            RetryPolicy::new(
                11,
                Duration::from_secs(10),
                Duration::from_millis(1),
                Duration::from_secs(1),
            ),
            Err(RetryError::InvalidPolicy)
        );
        assert_eq!(
            RetryPolicy::new(
                3,
                Duration::from_secs(10),
                Duration::from_secs(31),
                Duration::from_secs(31),
            ),
            Err(RetryError::InvalidPolicy)
        );
    }

    #[tokio::test]
    async fn a_non_retryable_mutation_timeout_is_reported_as_uncertain() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/submit").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(
            TransportSettings::default()
                .without_system_proxy()
                .with_request_timeout(Duration::from_millis(25)),
        )
        .expect("HTTP client");
        let client = RetryClient::new(transport, RetryPolicy::default());

        Mock::given(method("POST"))
            .and(path("/submit"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_delay(Duration::from_millis(250)),
            )
            .mount(&server)
            .await;

        let result = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::POST,
                    &target,
                    RetryOperation::new(OperationRetryability::NonRetryable)
                        .with_operation_id(OperationId::new("job-456").expect("operation ID")),
                )
                .with_body("application/json", b"{}"),
                CancellationToken::new(),
            )
            .await;
        let error = match result {
            Ok(_) => panic!("timed out mutation unexpectedly returned a response"),
            Err(error) => error,
        };

        assert!(matches!(
            &error,
            RetryError::OutcomeUncertain {
                cause: OutcomeUncertainty::Transport(ApiError::Timeout),
                ..
            }
        ));
        assert_eq!(
            error.operation_id().map(OperationId::as_str),
            Some("job-456")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retry_wait_stops_at_the_total_deadline() {
        let cancellation = CancellationToken::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let wait_cancellation = cancellation.clone();
        let sleeper = tokio_sleeper();
        let wait = tokio::spawn(async move {
            wait_before_retry(
                &wait_cancellation,
                deadline,
                Duration::from_secs(1),
                &sleeper,
            )
            .await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(2)).await;

        assert_eq!(
            wait.await.expect("wait task"),
            Err(RetryError::DeadlineExceeded)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retry_wait_is_cancelled_without_waiting_for_its_delay() {
        let cancellation = CancellationToken::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let wait_cancellation = cancellation.clone();
        let sleeper = tokio_sleeper();
        let wait = tokio::spawn(async move {
            wait_before_retry(
                &wait_cancellation,
                deadline,
                Duration::from_secs(5),
                &sleeper,
            )
            .await
        });
        tokio::task::yield_now().await;
        cancellation.cancel();

        assert_eq!(wait.await.expect("wait task"), Err(RetryError::Cancelled));
    }

    #[tokio::test]
    async fn honors_retry_after_for_a_conditionally_retryable_mutation() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let target = origin.request_url("/submit").expect("request URL");
        let token = BearerToken::new("unit-test-token").expect("test token");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let policy = RetryPolicy::new(
            2,
            Duration::from_secs(10),
            Duration::ZERO,
            Duration::from_secs(5),
        )
        .expect("valid retry policy");
        let delays = Arc::new(Mutex::new(Vec::new()));
        let delays_for_sleeper = Arc::clone(&delays);
        let sleeper: Sleeper = Arc::new(move |delay| {
            delays_for_sleeper
                .lock()
                .expect("retry delays lock")
                .push(delay);
            Box::pin(std::future::ready(()))
        });
        let client = RetryClient::with_seed_and_sleeper(transport, policy, 42, sleeper);
        let response_count = Arc::new(AtomicUsize::new(0));
        let response_count_for_mock = Arc::clone(&response_count);
        Mock::given(method("POST"))
            .and(path("/submit"))
            .and(body_string("{}"))
            .respond_with(move |_: &Request| {
                if response_count_for_mock.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(StatusCode::FORBIDDEN)
                        .insert_header("Retry-After", "2")
                        .set_body_json(serde_json::json!([{
                            "errorCode": "REQUEST_LIMIT_EXCEEDED",
                            "message": "API request limit reached"
                        }]))
                } else {
                    ResponseTemplate::new(StatusCode::OK)
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let response = client
            .execute(
                retry_request(
                    &origin,
                    &token,
                    Method::POST,
                    &target,
                    RetryOperation::new(OperationRetryability::Conditional),
                )
                .with_body("application/json", b"{}"),
                CancellationToken::new(),
            )
            .await
            .expect("rate-limited operation is retried");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            *delays.lock().expect("retry delays lock"),
            [Duration::from_secs(2)]
        );
    }
}
