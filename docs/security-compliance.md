# Security, Supply Chain, and Compliance Requirements

**Status:** Required controls for the planned native implementation, not a claim
of an implemented security system or compliance certification.

Read with the [architecture](technical-architecture.md),
[development requirements](development-setup.md), and
[security test cases](testing-strategy.md).

## 1. Trust Model

Protect Salesforce access/refresh tokens, private keys, org data, project files,
and release artifacts. Untrusted inputs include command arguments, configuration,
remote JSON/XML, downloaded archives, and any future external executable.

The CLI runs with the invoking user's privileges. Rust memory safety does not
prevent credential leakage, unsafe paths, injection, excessive resource use, or
an authorized command targeting the wrong org. Native plugins would run with
those same privileges and are not a sandbox.

The default distribution MUST have no Node.js runtime or automatic legacy CLI
fallback. Unsupported operations must fail explicitly instead of executing an
unexpected installed tool.

## 2. Credentials and State Ownership

**Upstream context:** The inspected core library uses home-scoped `.sf`/`.sfdx`
state, encrypted fields in per-user authentication files, and platform-specific
key storage with generic fallbacks. See
[`global.ts`](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/global.ts),
[`authInfoConfig.ts`](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/config/authInfoConfig.ts),
and [`keyChain.ts`](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/crypto/keyChain.ts).
This is not equivalent to placing every token directly in an OS credential store.
The native policy below deliberately chooses separate state and no automatic
file-based fallback; it must not be advertised as upstream storage compatibility.

- Persist interactive credentials only through a supported OS secure store using
  `keyring`: macOS Keychain, Windows Credential Manager, or an available Linux
  secret store. Configure and test the actual backend, not merely the crate name.
- Namespace entries for `sf-rust` and the relevant account/org. Do not overwrite
  credentials belonging to the official CLI or another installation.
- A locked, missing, or unsupported credential store MUST produce an actionable
  error. Never fall back to plaintext JSON or pretend authentication was saved.
- Headless CI must have an explicit ephemeral authentication mode using a
  secret-injected token or supported key-based flow. Tokens may arrive via stdin
  or a documented environment variable; neither mode may silently persist them.
  Environment variables can still leak to child processes or diagnostics.
- Never ask developers to put passwords or tokens in `~/.sf/config.json`.
  Configuration holds non-secret preferences; authentication has a separate
  storage lifecycle.
- Use `secrecy`/`zeroize` for supported secret-containing types, minimize copies,
  and avoid `Debug`/serialization paths that reveal them. Zeroization is
  defense-in-depth: it cannot promise removal of allocator copies, library
  buffers, OS caches, swap, or crash dumps.
- The native state store is separate from upstream state. Reading non-secret
  upstream configuration is an explicit compatibility/import operation. Do not
  silently decrypt, migrate, or rewrite upstream auth files.
- State writes need restrictive permissions or equivalent Windows ACLs,
  same-filesystem temporary files, atomic replacement where supported, and
  concurrency control. Validate paths and handle symlinks deliberately. Report
  failed writes rather than leaving a successful-looking partial operation.
- Logout/delete must remove the native credentials and metadata required by that
  command's contract. Distinguish local credential deletion from remote token
  revocation; do not claim one guarantees the other.

Explicit credential import or encrypted file-based headless storage is deferred.
Either requires a separately reviewed format, key lifecycle, migration design,
and tests before being offered as a supported capability.

## 3. Authentication and Secret-Bearing Output

OAuth browser login must validate callback state, use PKCE where supported by the
selected flow, bind any local callback listener to loopback only, and have a
deadline and cancellation cleanup. Validate login/redirect origins and request
only scopes required for supported workflows. Use well-maintained protocol
libraries rather than inventing cryptography.

Private keys must not be copied into general config or exposed in process
arguments. Serialized refresh and credential updates must avoid races between
simultaneous commands. Refresh failures must not trigger unbounded reauthentication
or retry loops.

No secrets may enter logs, traces, error chains, telemetry, snapshots, or build
artifacts. Redact authorization headers, cookies, OAuth URLs carrying secrets,
and sensitive request/response fields at their owning interface; regex filtering
of final log text alone is insufficient.

Some upstream commands intentionally display credentials. Such a command can be
supported only with an explicitly reviewed, documented secret-bearing output
contract and synthetic test fixtures. Its intentional stdout result is the narrow
exception to output redaction, not permission to log it. Do not silently redact
a supported command's required JSON fields while claiming parity, and do not
introduce surprise credential output in unrelated commands.

## 4. Network and API Safety

- Require HTTPS with TLS 1.2 or newer and certificate/hostname verification.
  Never ship a general "disable TLS verification" escape hatch.
- Support documented enterprise proxy and CA configuration through the chosen
  TLS implementation, with tests. Browser/system trust and bundled root stores
  are not automatically identical. Certificate pinning is not a default policy;
  it introduces certificate-rotation and availability risks.
- Bind authorization to the validated org instance origin. Cross-origin redirects,
  pagination links, or URLs in server responses must not receive credentials
  automatically. Reject unexpected schemes/origins instead of trusting arbitrary
  URLs from JSON or XML.
- Bound connection/request timeouts, response sizes, concurrency, retries, and
  polling duration. Reuse connections without allowing unbounded buffered data.
- Respect API limit information and `Retry-After` where applicable. Use bounded
  backoff with jitter for transient, safely retryable requests. A fixed local
  rate limiter alone cannot know all per-org and shared-client quotas.
- Never retry a mutation merely because the connection timed out. Use an actual
  idempotency guarantee or a recorded operation identifier and status lookup.
  If acceptance is uncertain, report that uncertainty with recovery instructions.
- Support interruption without claiming rollback of a remote operation. Preserve
  job IDs safely so the user can resume reporting or request cancellation.

HTTP loopback servers are permitted only through test configuration. Production
authentication must not inherit a test-only transport bypass.

## 5. Files, XML, Archives, and Processes

- Serialize XML with a library that escapes values and handles namespaces; do not
  concatenate raw user input into SOAP envelopes. Recognize SOAP faults even when
  the HTTP status is successful.
- Disable external entity/DTD resolution and enforce practical limits for XML
  depth, text size, total response size, and parsing work.
- Validate archive entry paths before extraction, including absolute paths,
  traversal, symlinks, and platform-specific path forms. Limit both compressed
  input and expanded size/count, and keep extraction inside its intended root.
- Treat retrieved metadata and project manifests as untrusted. Do not execute
  hooks, scripts, or project-supplied binaries as a side effect of reading them.
- Launch necessary subprocesses with argument arrays, never shell-concatenated
  user input. Do not expose secrets through arguments or inherited environments.
- External native plugins are deferred. Before enabling them, require explicit
  trusted registration, versioned protocol negotiation, time/output limits, safe
  environment handling, and clear permission/credential exposure. Never silently
  discover and execute a binary from an untrusted working directory.

## 6. Dependency and Release Integrity

- Commit `Cargo.lock` and build with `--locked`. The lockfile fixes dependency
  resolution and records registry package checksums; it does not by itself
  guarantee trustworthy code, a secure build, or bit-for-bit reproducibility.
  The `cargo-lock` library is not a replacement for this policy.
- Pin the Rust toolchain, CI actions, and auxiliary tools. Review build scripts,
  procedural macros, default features, and platform-native dependencies as code
  executed or distributed by the build.
- Use `cargo audit` for RustSec advisories and `cargo deny check` for the approved
  license/source/advisory policy. Do not describe advisory scanning as CVE-only.
  Exceptions need a reason, owner, expiry, and mitigation.
- Restrict dependency sources. Git dependencies require immutable revisions and
  justification; unexpected registries or sources must fail the configured check.
- CI tokens must have minimum permissions. Never expose signing keys or Salesforce
  credentials to untrusted pull-request code, logs, caches, or uploaded artifacts.
- Publish checksums, build provenance, and a dependency/license inventory with
  release artifacts. Verify any automatic update before replacing an executable;
  self-update is deferred until that trust and rollback model exists.
- Document platform requirements and test the packaged executable. Reproducible
  builds may be claimed only after independent rebuilds establish them for the
  declared environment, not merely because `Cargo.lock` is committed.

## 7. Licensing, Data Handling, and Operational Readiness

The inspected [CLI license](https://github.com/salesforcecli/cli/blob/8ca3ae36d11a01726aec741897547adec3aa5ad2/LICENSE.txt)
is Apache-2.0. The inspected
[core](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/LICENSE.txt),
[command base](https://github.com/salesforcecli/sf-plugins-core/blob/bc6bc6f14add235cdc739a38eb244c404c2d587c/LICENSE.txt),
[deploy/retrieve plugin](https://github.com/salesforcecli/plugin-deploy-retrieve/blob/2e867d60f01a148fac1cbaf469136fed3314eaf3/LICENSE.txt),
and [SDR](https://github.com/forcedotcom/source-deploy-retrieve/blob/b8998b38cf5b4d3278f5fb1f964b96d5a47886d7/LICENSE.txt)
licenses also identify Apache-2.0. This source snapshot does not constitute a
license inventory of all plugins, transitive dependencies, or non-code assets.

Follow the actual licenses of the upstream CLI, each ported plugin/library, and
all Rust dependencies. Inspect the license at the pinned revision rather than
assuming that every Salesforce repository has the same terms. Preserve required
copyright, license, and attribution notices for reused or adapted material.
Track modified upstream-derived files and inspect applicable `NOTICE` files before
distribution; a root license check alone does not establish all obligations.

Select this project's distribution license before publishing code; reimplementation
does not automatically settle licensing obligations. Do not imply that this is an
official or Salesforce-endorsed distribution. Track source provenance for ports
and reviewed compatibility fixtures.

Use synthetic or explicitly sanitized org data in tests. Org URLs, usernames,
record contents, and identifiers can be sensitive even when they are not tokens.
Keep live-test credentials short-lived and scoped; clean up org resources after
success and failure. Do not enable telemetry by default or send org data to
unrelated services.

Before public release, publish a private vulnerability-reporting channel and a
process for dependency updates, credential exposure, revocation guidance, and
patched releases. Security controls must have negative tests and named release
checks in the [testing strategy](testing-strategy.md); a compliance heading alone
is not an assurance or certification.

## 8. Third-Party Dependency Policy and Review

The `deny.toml` file records the workspace's supply chain control policies:

- **Advisories**: The RustSec advisory database is fetched daily in CI and checked
  against the locked dependency set. All known vulnerabilities are denied; no
  silent transitive exposure is allowed.
- **Licenses**: All dependencies must declare a license. Widely-used permissive
  licenses compatible with Apache-2.0 (MIT, BSD, ISC, Unlicense, MPL-2.0) are
  allowed; copyleft licenses (GPL, AGPL) and proprietary restrictions are denied.
  See the [upstream baseline](../compatibility/upstream-baseline.yaml) and deny
  rules for detail.
- **Sources**: Dependencies must come from the Rust registry (crates.io) only;
  git dependencies and unknown registries are denied unless explicitly reviewed.
- **Bans**: Multiple versions of a crate are warned; future bans may restrict
  problematic crates.

### Advisory Exceptions

The RustSec advisory database occasionally includes advisories that affect
transitive dependencies in ways that:
- The current version is not yet released with a fix.
- The fix requires upstream intervention or a longer upgrade path.
- The vulnerable code path is not reachable by this project.

Advisory exceptions require:
1. **Owner**: Named project stakeholder responsible for tracking the exception.
2. **Expiry**: Calendar date by which the exception must be reviewed or removed.
3. **Reason**: Clear statement of the reachability assessment or mitigation.
4. **Replacement**: The date the exception was added and the lock file revision.

Record exceptions in `deny.toml` under `advisories.ignore` with the advisory ID.
Do not add exceptions without naming an owner and expiry date. Upon expiry,
assess whether a fix has been released and commit a removal; if not resolved,
renew with an updated date or escalate.

Example (not active):
```toml
[advisories]
ignore = [
  { id = "RUSTSEC-2024-00001", reason = "pending upstream release of library", expires = "2025-01-01", owner = "example@example.com" }
]
```

### Tool Versions and Installation

The CI workflow pins exact versions of audit and policy tools to ensure
reproducible scans. Developers must use matching versions for local checks.
See the [development setup](development-setup.md#6-setup-and-required-developer-checks)
for pinned `cargo-audit`, `cargo-deny`, and `cargo-llvm-cov` versions and
installation commands with `--locked`.

Tool upgrades must be:
1. Tested against the current dependency set in a separate PR.
2. Reviewed by the project owner before committing.
3. Updated in `deny.toml`, `development-setup.md`, and `.github/workflows/ci.yml`
   in a single change.
4. Documented in the commit message with any policy or behavior changes.

Automatically selecting the latest tool version (e.g., omitting `--version` or
`--locked`) defeats reproducibility and introduces unexpected policy changes
without review.
