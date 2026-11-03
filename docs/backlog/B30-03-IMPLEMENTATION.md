# B30-03: Safe Retry and Rate-Limit Policy

## Result

`sf_api::retry::RetryClient` executes a buffered `RetryRequest` only according
to the caller's `OperationRetryability`; it never infers replay safety from the
HTTP method. `Safe` operations may retry transport timeout/request/response-read
failures and HTTP 408, 429, 500, 502, 503, or 504. `Conditional` operations
retry only after an explicit `429` or Salesforce `REQUEST_LIMIT_EXCEEDED`
response; transport failures and generic server errors are not retried. Other
operations are not retried.

The default policy allows three total attempts, a 60-second total deadline, a
250 ms initial exponential-backoff cap, and a 10-second maximum delay.
Configurable policies remain bounded to ten attempts, ten minutes, and
30-second backoff. Backoff uses full jitter from a seeded generator. A valid
`Retry-After` delay or HTTP date is honored when it fits the configured delay
bound; if it does not, the last HTTP response is returned without an early
retry. Org-wide and per-application `Sforce-Limit-Info` usage metadata is
parsed; retries stop when either reported quota is exhausted.

Cancellation and the total deadline cover both requests and retry waits.
Cancellation/deadline expiry or ambiguous transport failure during a
non-safe in-flight operation returns `RetryError::OutcomeUncertain`; a
validated, caller-supplied `OperationId` is preserved when available. The
executor does not retry such writes. Request bodies are buffered bytes and are
replayed unchanged only when the declared operation semantics permit it.

## Workflow boundary

The retry executor is opt-in so workflow owners must choose operation semantics
deliberately. No command currently selects the policy: `sf data query` remains
planned until CLI credential/session wiring is available. The module preserves
known operation IDs but does not invent an ID or perform workflow-specific
status lookup; callers must provide the recovery mechanism for their operation.

## Verification

Mock-server tests cover maximum attempts, explicit retry classification
independent of HTTP method, conditional mutation retry after
`REQUEST_LIMIT_EXCEEDED`, `Retry-After` delay and upper bounds, exhausted API
quota suppression, seeded jitter reproducibility, deadline and cancellation
behavior, and no replay after ambiguous mutation timeout/cancellation. Tests
assert that uncertain outcomes retain validated operation IDs. Deterministic
seeded-jitter and delay-capture tests plus Tokio-clock tests exercise retry
timing, wait cancellation, and deadlines; all network responses use synthetic
loopback servers.
