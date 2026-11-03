# B20-03: Org Selection and Auth Lifecycle

## Result

Implemented the native authentication core for one selected OAuth flow:
authorization-code login with mandatory PKCE S256. `OAuthFlow` creates a
cryptographically random state/verifier pair, binds its one-shot callback
listener to `127.0.0.1`, validates callback state, bounds request size and
duration, and drops the listener on completion or cancellation. Token exchange,
refresh, and revocation use HTTPS Salesforce endpoints, bounded requests, and
single attempts; response bodies and token values are not copied into errors.

`SessionManager` refreshes expiring credentials under a shared per-org async
lock, re-reads credentials after acquiring the lock, persists the refreshed
token through `CredentialBackend`, and surfaces refresh/storage failures. It
does not retry or start a reauthentication loop. If the provider omits expiry
metadata, expiry stays unknown instead of being fabricated. Remote logout
attempts local deletion even if revocation fails and reports that partial result.

`OrgDirectory` provides stable listing and selector resolution with this
precedence:

1. Explicit selector
2. Configured default
3. Credential marked as the stored default

Each selector checks alias, then username, then Salesforce org ID. Missing and
ambiguous matches are errors; an invalid higher-precedence selector never
falls back.

## Compatibility and support boundary

The inventory records the pinned first-release auth/org command candidates and
their status. The selected native flow requires an explicit client ID; this is
an intentional limitation until a reviewed native application client is chosen.
No CLI command is marked `supported`: browser launching, OS secure-store
adapters, identity lookup, persistent login wiring, and complete command-level
JSON/exit contracts are not available in this slice. `sf org list` now returns
the standard explicit unsupported-command error rather than an empty success.
The in-memory credential backend remains test/ephemeral only. Live login was
not run; any future live test must use an opt-in disposable least-privilege org.

Refresh serialization is process-local. A future persistent secure-store
adapter must add cross-process coordination before simultaneous CLI processes
can safely refresh the same credential.

## Verification

The synthetic `auth_lifecycle` integration tests cover:

- Refresh success, persistence, failure without retry, and missing refresh token.
- A refresh response without expiry metadata leaves expiry unknown.
- Concurrent expiry refresh through separate session-manager instances.
- Local logout, successful revocation, and explicit partial outcomes when remote
  revocation or local deletion fails.
- Explicit selector/configured-default/stored-default precedence and wrong-org
  selection.
- Unavailable credential storage as an error rather than an empty result.
- PKCE S256 generation, loopback-only callback, state mismatch, successful
  callback, timeout, and cancellation.

`sf-cli` argument contract tests verify that incomplete org-list behavior fails
with the explicit unsupported exit code, including JSON-mode error output.
