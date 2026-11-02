# 3. Salesforce API Transport

**Goal:** implement bounded, authenticated transport primitives that workflow
modules can use without knowing HTTP/XML implementation details.

**Depends on:** workspace foundation and authenticated-session interface from
configuration/authentication.

## B30-01: Build the authenticated HTTP client

- [ ] Configure a reusable client with explicit TLS backend, certificate
  validation, proxy/CA behavior, and bounded connection/request timeouts.
- [ ] Attach authorization only to the validated instance origin. Reject
  unexpected schemes, origins, and credential-forwarding redirects.
- [ ] Bound response size and concurrency. Keep blocking operations off async
  executor threads.
- [ ] Provide injectable transport settings for loopback tests without exposing
  a production TLS bypass.

**Likely files**

- `crates/sf-api/src/client.rs`
- `crates/sf-api/src/origin.rs`
- `crates/sf-api/src/error.rs`
- `crates/sf-api/tests/http_client.rs`

**Acceptance**

- Mock-server tests cover TLS/redirect/origin policy, timeout, malformed/large
  response, and auth header handling.
- Tests establish that credentials are not forwarded to a second origin.
- Logs and error chains omit authorization and sensitive bodies.

## B30-02: Implement REST requests, pagination, and API errors

- [ ] Add typed request/response models for the first selected REST workflows.
- [ ] Parse Salesforce errors into typed safe categories while retaining useful
  request identifiers and retry guidance.
- [ ] Validate pagination next links, preserve meaningful ordering, and stream
  large results where the command contract permits.
- [ ] Keep API-version selection explicit and per workflow.

**Likely files**

- `crates/sf-api/src/rest/mod.rs`
- `crates/sf-api/src/rest/query.rs`
- `crates/sf-api/src/rest/pagination.rs`
- `crates/sf-api/tests/rest_query.rs`
- `crates/sf-api/tests/fixtures/`

**Acceptance**

- Wire tests cover empty/multiple pages, cross-origin next link, API errors,
  malformed JSON, and bounded results.
- `sf data query` (if selected) has a working vertical slice rather than a
  placeholder and passes differential tests.

## B30-03: Add safe retry and rate-limit policy

- [ ] Classify operations as safe to retry, conditionally retryable, or
  non-retryable. Do not infer safety from HTTP method alone.
- [ ] Respect applicable `Retry-After` and platform limit information with
  bounded backoff, jitter, deadline, and cancellation.
- [ ] Surface uncertain mutation outcomes and operation IDs instead of replaying
  writes after an ambiguous timeout.

**Likely files**

- `crates/sf-api/src/retry.rs`
- `crates/sf-api/src/limits.rs`
- `crates/sf-api/tests/retry_policy.rs`

**Acceptance**

- Deterministic-clock tests verify maximum retries, delay, deadline, cancellation,
  and non-replay after uncertain mutation acceptance.

## B30-04: Add Metadata SOAP and XML handling when needed

- [ ] Implement typed SOAP envelope/fault encoding and decoding with namespaces
  and escaped values; never construct XML by raw string interpolation.
- [ ] Enforce parser depth/body limits and disable external entity resolution.
- [ ] Add SOAP-fault handling even on successful HTTP status.

**Likely files**

- `crates/sf-api/src/metadata/soap.rs`
- `crates/sf-api/src/metadata/xml.rs`
- `crates/sf-api/tests/metadata_soap.rs`
- `crates/sf-api/tests/fixtures/`

**Acceptance**

- Synthetic fixtures cover XML escaping/namespaces, faults, malformed and
  oversized bodies, invalid encoding, and parser resource limits.
- Property tests cover agreed round-trip invariants; fuzzing is added when parser
  surface and CI runtime are known.
