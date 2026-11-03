# B30-01: Authenticated HTTP Client

## Result

`sf-api` now provides a reusable `HttpClient` with explicit Rustls and WebPKI
roots, optional system or explicit proxy configuration, and additive PEM CA
roots. Connection and total request deadlines, response size, concurrency,
certificate count, and certificate size are bounded. Redirects are disabled.
Defaults are 10 seconds to connect, 60 seconds for an end-to-end request,
16 MiB per response, and 8 concurrent requests; configured bounds are capped at
10 minutes, 64 MiB, and 64 concurrent requests.

`InstanceOrigin` accepts only credential-free HTTPS Salesforce origins. Every
request target is checked against that exact origin before an authorization
header is constructed. `BearerToken` validates and redacts its debug output;
typed errors omit raw transport errors, URLs, response bodies, and credentials.
Responses are fully buffered only after enforcing the configured size limit,
and expose sanitized JSON deserialization errors.

Unit tests use a private `cfg(test)` constructor limited to HTTP loopback
addresses. It does not expose a production HTTP-origin or TLS-verification
bypass. Production origins remain HTTPS-only.

## Verification

`sf-api` integration and mock-server tests cover Salesforce origin validation,
same-origin target resolution, bearer-header delivery, cross-origin rejection,
redirect rejection without a request to the second origin, request deadlines,
concurrency, oversized bodies, malformed JSON, proxy configuration, invalid CA
configuration, and sanitized failures.

The reqwest workspace dependency disables implicit defaults and explicitly
selects Rustls with WebPKI roots, HTTP/2, JSON, and system-proxy support.
