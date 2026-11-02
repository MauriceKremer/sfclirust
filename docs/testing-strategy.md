# Testing, Compatibility, and Release Gates

**Status:** Required test design. No test suite or CI workflow exists yet.
These requirements become enforceable only when the bootstrap milestone wires
them into required repository checks.

Read with the [architecture](technical-architecture.md),
[Rust development requirements](development-setup.md), and
[security requirements](security-compliance.md), and follow the ordered
[implementation backlog](backlog/README.md).

## 1. Define Compatibility Before Implementing a Command

The target is phased first-party command parity, not an unqualified claim that
every Salesforce CLI plugin already works. The shipped executable must remain
native-only; Node.js is allowed for the isolated upstream test oracle, never as
a runtime dependency or hidden fallback.

Before porting a command, add a machine-readable compatibility inventory entry
with:

- Canonical command path, aliases, owning upstream plugin, and exact reference
  CLI/plugin versions and source revisions.
- Upstream delivery category (bundled, JIT, or framework), and an explicit reason
  for deferral or exclusion. Do not silently drop JIT commands from parity counts.
- Positional arguments, flags, defaults, environment/config precedence, and
  required project/org context.
- Supported operating systems and feature variants.
- Success and failure cases, exit codes, stdout/stderr behavior, JSON shape,
  side effects, prompts, and interruption behavior.
- Documented normalizations or intentional differences, each with a reason.
- Status: `planned`, `in-progress`, `supported`, or `intentionally-unsupported`.
  A command is `supported` only when its required contract tests pass.

The baseline procedure and source ownership are defined in the
[architecture](technical-architecture.md). A new upstream release does not
silently change the oracle; baseline upgrades are reviewed changes.

## 2. Test Layers

| Layer | Scope and evidence |
|---|---|
| Unit | Test meaningful behavior in every crate: parsing/validation, config precedence, state transitions, error mapping, pagination decisions, XML mappings, and retry classification. |
| Module integration | Exercise real production interfaces with a local `wiremock` server, temporary filesystem, and fake credential storage. Verify request paths, headers, bodies, and observable results, not just internal method calls. |
| Executable | Use `assert_cmd` to launch the built `sf-rust` binary. Assert exit status, stdout, stderr, JSON parsing, redirected input/output, and filesystem changes. |
| Differential | Execute the pinned upstream CLI and Rust CLI against equivalent isolated inputs; compare the declared contract rather than assuming byte-identical output. |
| Live | Use disposable, least-privilege Salesforce test orgs for protocols and platform behavior mocks cannot establish. Never use a developer's personal or production org. |

Prefer tests through the same interface as production callers. Introduce a fake
adapter at an actual seam rather than building a mock framework around every
function. Do not write a custom test runner when Cargo and existing assertion
tools suffice.

Ordinary `cargo test --workspace --locked` MUST need neither Salesforce credentials
nor external network access after dependencies are available. Tests may use
loopback servers, but must isolate state directories, environment, clock, and
credential storage. Use child-process environments rather than concurrently
mutating the test runner's global environment. Unit tests must not read or write
the user's actual `~/.sf`, `~/.sfdx`, or OS credential store.

## 3. Differential Test Rules

1. Install the exact approved upstream distribution into a separate test location;
   record its plugin inventory and executable path. Never resolve either executable
   through an ambiguous `PATH`.
2. Isolate both CLIs' home/state directories and project copies. Do not let them
   concurrently modify the same authentication files or org resources.
3. Use synthetic fixtures for offline comparisons. For live writes, create
   separate equivalent resources/orgs or reset verified state between runs;
   do not repeat a destructive command against a shared target.
4. Compare JSON structurally: types, field presence, null versus absent, array
   order where meaningful, warnings, error categories, and command-specific
   envelope shape. Check process exit status independently of JSON fields.
5. Normalize only explicitly listed volatile values, such as a test-generated
   identifier or timestamp. Do not delete all IDs/errors/warnings or sort every
   array merely to make a comparison pass.
6. Assert stdout and stderr separately. JSON output must not contain progress
   bars, ANSI escapes, diagnostic logs, or multiple unrelated documents.
   Include JSON errors on stdout and the `SF_CONTENT_TYPE=JSON` selection path
   where applicable, as established by the
   [upstream command base](https://github.com/salesforcecli/sf-plugins-core/blob/bc6bc6f14add235cdc739a38eb244c404c2d587c/src/sfCommand.ts).
7. Test non-TTY execution and the command's prompt/confirmation policy. A command
   that needs input must not hang indefinitely when stdin is unavailable.
8. Snapshot expected output only after review. Fixture updates must identify
   whether upstream changed, a defect was fixed, or compatibility was intentionally
   changed. Sanitize secrets before persisting a fixture, not after committing it.

Human-readable output needs usable, tested semantics and documented differences.
Exact help wording, stack traces, and platform-specific formatting are not
automatically compatibility promises. JSON and exit behavior remain explicit
per-command contracts, not assumed universal envelopes.

## 4. Required Failure and Recovery Scenarios

- Unknown commands/flags, invalid identifiers/URLs, missing org/project context,
  configuration precedence conflicts, malformed state, and unsupported commands.
- Expired access tokens, successful and failed refresh, revoked authorization,
  concurrent refresh requests, unavailable credential stores, and cancelled login.
- Timeouts, DNS/TLS failures, redirects, rate limits, `Retry-After`, transient
  server errors, malformed/oversized responses, and a connection lost after a
  server may have accepted a write.
- Pagination across empty and multiple pages, invalid or cross-origin next links,
  and bounded streaming of large results.
- SOAP faults in successful HTTP responses, XML namespaces/escaping, invalid
  encoding, truncated archives, path traversal, symlinks, and decompression limits.
- Deployment validation errors, partial failure, async job IDs, bounded polling,
  interrupted waits, resumable reporting, and best-effort remote cancellation.
- Concurrent config updates, write interruption, disk full, denied permissions,
  Windows path behavior, missing trust roots, and headless authentication.
- No secret leakage through output, tracing, errors, snapshots, or subprocess
  arguments. Explicit secret-bearing commands require separate output tests.

Use deterministic clocks and seeded random generators for timing/retry tests.
Assert retry counts and deadlines; tests that merely eventually succeed cannot
prove that a retry budget or timeout is respected.

## 5. Property Tests, Fuzzing, and Coverage

Use `proptest` for generated invariants: supported XML round trips, serialization
stability, identifier validation, config merge precedence, and state transitions.
Keep regression cases for every discovered failure. Property-based testing is not
the same as coverage-guided fuzzing.

Add separate `cargo-fuzz` targets for untrusted XML/JSON/archive handling when
those parsers are implemented. The fuzz toolchain may use a separately pinned
nightly; it must not change the application's stable compiler requirement.
Seed with small synthetic examples and keep minimized crash reproducers. Include
size/time limits: "does not panic" alone is insufficient for resource exhaustion.

Require at least **80% line coverage of `sf-core` and `sf-api` library code**:

```sh
cargo llvm-cov -p sf-core -p sf-api --lib --locked --fail-under-lines 80
```

Pin `cargo-llvm-cov` and provision the matching LLVM tools in the coverage job.
Any exclusion needs a reviewed explanation; do not exclude difficult auth/error
paths to meet the threshold. Independently require positive and negative tests
for every supported command and security-sensitive workflow. Aggregate coverage
does not establish compatibility or correctness.

## 6. Required CI Jobs

| Job | Gate |
|---|---|
| Formatting and linting | Run the exact check-mode formatting, all-targets Clippy with `-D warnings`, and rustdoc commands in the [development guide](development-setup.md). |
| Default build and tests | Locked workspace check/test on each supported OS. Include doctests and executable integration tests; do not accidentally exclude doctests with an all-targets-only test invocation. |
| Features | Run checks, Clippy, and tests for the default set and each supported optional configuration. Include `--no-default-features` if supported. Use `--all-features` only when the features are valid together; explicitly enumerate mutually exclusive combinations. |
| MSRV | Use the manifest's exact declared MSRV for locked workspace checks/tests across supported configurations; document any test-tool-only compiler constraint rather than quietly raising production MSRV. |
| Dependencies | `cargo audit` and `cargo deny check` with reviewed advisory/license/source policy. Policy exceptions require rationale, owner, and expiry; stale advisory data or a failed scan must not appear as success. |
| Coverage | Enforce the threshold above and retain a report. |
| Offline compatibility | Execute all offline contracts for commands marked `supported` against the pinned oracle or reviewed fixtures tied to that oracle. |
| Live compatibility | Protected, explicitly authorized workflow for disposable orgs. Required before releasing affected auth/API/metadata capabilities; never expose credentials to untrusted pull requests. |
| Release matrix | Build and execute packaged binaries on every advertised target architecture, and run tests appropriate to its TLS/credential implementation. A cross-build alone is insufficient. |

Pin reusable actions to immutable revisions and tools to exact releases. Test the
shipping release profile as well as development builds. Required repository checks
must block merge; do not allow failed checks through blanket `continue-on-error`.

Live tests must be excluded from default offline tests with explicit test metadata.
Selecting the live suite without its required credentials is a failure, not a
successful skip. Every run needs a unique resource prefix, cleanup on success and
failure, and a documented TTL/reconciliation procedure for orphaned scratch orgs.

## 7. Performance and Release Acceptance

Measure release binaries on fixed runners and synthetic datasets. Record cold and
warm startup (`--help`, `--version`, local config reads), peak memory, artifact
size, large-query streaming, and representative metadata operations. Separate
local processing from network/server time.

The first implementation milestone must record reference hardware, input sizes,
sample counts, and approved budgets. Later changes must compare against those
budgets; do not claim a speedup over Node.js without repeatable equivalent work.
Performance cannot justify missing validation, compatibility, or safe retry rules.

A release requires:

- Green required checks and a current compatibility inventory.
- Live evidence for the capabilities being released, plus native smoke tests of
  the actual packaged artifacts, not just an earlier test build.
- Dependency/license review, checksums, provenance, and documented installation
  and rollback behavior as specified in [security](security-compliance.md).
- Release notes listing supported commands, intentional differences, known limits,
  platform requirements, and the precise upstream baseline.

Until these artifacts and workflows exist, this document is a test specification,
not evidence that the project passes its gates.

## 8. Primary References

- [Upstream source map and baseline procedure](technical-architecture.md#2-upstream-baseline-and-porting-method).
- [Upstream command execution and JSON handling](https://github.com/salesforcecli/sf-plugins-core/blob/bc6bc6f14add235cdc739a38eb244c404c2d587c/src/sfCommand.ts)
  and [structured error output](https://github.com/salesforcecli/sf-plugins-core/blob/bc6bc6f14add235cdc739a38eb244c404c2d587c/src/SfCommandError.ts).
- [Rust test organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html)
  and [Cargo test targets/doctests](https://doc.rust-lang.org/cargo/commands/cargo-test.html).
- [Cargo CI guidance](https://doc.rust-lang.org/cargo/guide/continuous-integration.html).
  The stricter merge/release gates in this document are project policy, not a
  claim that Cargo automatically supplies them.
