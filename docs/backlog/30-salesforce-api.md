# 3. Salesforce API Transport

**Goal:** implement bounded, authenticated transport primitives that workflow
modules can use without knowing HTTP/XML implementation details.

**Depends on:** workspace foundation and authenticated-session interface from
configuration/authentication.

## B30-01: Build the authenticated HTTP client

- [x] Configure a reusable client with explicit TLS backend, certificate
  validation, proxy/CA behavior, and bounded connection/request timeouts.
- [x] Attach authorization only to the validated instance origin. Reject
  unexpected schemes, origins, and credential-forwarding redirects.
- [x] Bound response size and concurrency. Keep blocking operations off async
  executor threads.
- [x] Provide injectable transport settings for loopback tests without exposing
  a production TLS bypass.

**Status: ✅ COMPLETE** — See [B30-01-IMPLEMENTATION.md](B30-01-IMPLEMENTATION.md).

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

- [x] Add typed request/response models for the first selected REST workflows.
- [x] Parse Salesforce errors into typed safe categories while retaining useful
  request identifiers and retry guidance.
- [x] Validate pagination next links, preserve meaningful ordering, and stream
  large results where the command contract permits.
- [x] Keep API-version selection explicit and per workflow.

**Status: ✅ COMPLETE** — See [B30-02-IMPLEMENTATION.md](B30-02-IMPLEMENTATION.md).
This status covers the transport library; the runnable `data query` acceptance
remains open in B30-05.

**Likely files**

- `crates/sf-api/src/rest/mod.rs`
- `crates/sf-api/src/rest/query.rs`
- `crates/sf-api/src/rest/pagination.rs`
- `crates/sf-api/tests/rest_query.rs`
- `crates/sf-api/tests/fixtures/`

**Acceptance**

- Wire tests cover empty/multiple pages, cross-origin next link, API errors,
  malformed JSON, and bounded results.
- The selected `sf data query` vertical slice and differential tests are owned
  by B30-05; transport tests alone do not meet that command acceptance.

## B30-03: Add safe retry and rate-limit policy

- [x] Classify operations as safe to retry, conditionally retryable, or
  non-retryable. Do not infer safety from HTTP method alone.
- [x] Respect applicable `Retry-After` and platform limit information with
  bounded backoff, jitter, deadline, and cancellation.
- [x] Surface uncertain mutation outcomes and operation IDs instead of replaying
  writes after an ambiguous timeout.

**Status: ✅ COMPLETE** — See [B30-03-IMPLEMENTATION.md](B30-03-IMPLEMENTATION.md).

**Likely files**

- `crates/sf-api/src/client.rs`
- `crates/sf-api/src/retry.rs`

**Acceptance**

- Deterministic-clock tests verify maximum retries, delay, deadline, cancellation,
  and non-replay after uncertain mutation acceptance.

## B30-04: Add Metadata SOAP and XML handling when needed

- [x] Implement typed SOAP envelope/fault encoding and decoding with namespaces
  and escaped values; never construct XML by raw string interpolation.
- [x] Enforce parser depth/body limits and disable external entity resolution.
- [x] Add SOAP-fault handling even on successful HTTP status.

**Status: ✅ COMPLETE** — See [B30-04-IMPLEMENTATION.md](B30-04-IMPLEMENTATION.md).

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

## B30-05: Deliver the authenticated query command

**Depends on:** B20-05 and B30-01/02/03. Independent of B40-03 after those
prerequisites, but required before B40-05.

- [ ] Verify the exact `data query` contract against the pinned source/oracle,
  including API version, target/default precedence, formats, pagination, errors,
  and supported flags. Correct candidate inventory assumptions before porting.
- [ ] Connect the production CLI to the shared persistent/ephemeral auth path
  and REST query workflow. Define bounded result handling for each supported
  output format; never return a truncated result as success.
- [ ] Add executable positive/negative and pinned compatibility tests, plus an
  opt-in disposable-org query in the authenticated user walkthrough.

**Likely files**

- `crates/sf-core/src/` (query workflow using the existing auth/REST interfaces)
- `crates/sf-cli/src/commands/data/`
- `crates/sf-cli/src/cli.rs`
- `crates/sf-cli/tests/` (query command contracts)
- `docs/compatibility/commands.yaml` and reviewed fixtures

**Acceptance**

- The built binary can query the org selected by a preceding persisted login,
  and can query with explicit ephemeral credentials without persisting them.
- Tests cover empty/multiple pages, configured output/resource limits, invalid
  SOQL, unknown org, refresh failure, timeout/interruption, exact JSON/exit
  contracts, and no success output after partial failure.
- Live query uses disposable least-privilege resources. Transport unit tests
  alone are not accepted as a runnable-command test.
