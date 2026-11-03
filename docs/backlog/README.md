# Implementation Backlog

This backlog turns the design specs into ordered work that can be implemented
and reviewed in small slices. It is the implementation source of truth until
the project adopts an issue tracker. Keep task IDs when tasks move to issues.

## Project constraints

- The shipped CLI is Rust-native. Node.js is permitted only to run the pinned
  upstream comparison oracle in isolated tests.
- No command is `supported` until its inventory entry and required tests pass.
- Do not add a placeholder command that exits successfully without doing the
  documented operation.
- Keep each crate's public interface small. Add a crate or adapter only for a
  demonstrated ownership, testability, or platform need.
- `sf-rust` is the development executable name. Do not overwrite or implicitly
  invoke the installed `sf`.
- Keep live Salesforce tests opt-in, isolated, least-privilege, and credential
  free in ordinary local and pull-request tests.

See [architecture](../technical-architecture.md),
[development requirements](../development-setup.md),
[testing strategy](../testing-strategy.md), and
[security requirements](../security-compliance.md). Those documents define the
normative behavior; this backlog defines work order and likely file ownership.

## Work order

| Order | Work package | Depends on | Outcome |
|---|---|---|---|
| 0 | [Workspace foundation](00-foundation.md) | None | A pinned, linted, tested Rust workspace and minimal executable. |
| 1 | [CLI contract and runtime](10-cli-runtime.md) | 0 | Stable parser, output, error, and process behavior for future commands. |
| 2 | [Configuration and authentication](20-state-auth.md) | 0, CLI contract from 1 | Isolated native state and testable org/auth workflows. |
| 3 | [Salesforce API transport](30-salesforce-api.md) | 0, auth/session interfaces from 2 | Safe, tested REST and SOAP transport primitives. |
| 4 | [Projects and metadata workflows](40-project-metadata.md) | 1, 2, 3 | Supported project loading and deploy/retrieve vertical slices. |
| 5 | [Parity and release](50-parity-release.md) | Each released workflow's package | First-party commands graduated through compatibility and release gates. |

Within a package, tasks are ordered unless marked independent. Tasks with unmet
dependencies stay unchecked. The workspace and several library slices now exist;
completed primitives are not evidence that their CLI workflows are usable.
Use the milestone sequence below for the remaining B40 work.

## B40 delivery target: runnable developer preview

Completing B40 must deliver a native `sf-rust` that can authenticate, reuse its
login from a new process, inspect the selected org, run a read-only query, and
deploy/retrieve the explicitly supported metadata types. It must also pass a
post-implementation review and all required CI checks on the delivered revision.
Finishing the mapping and transport libraries alone does not meet this target.

This is a build-from-source developer preview, not full Salesforce CLI parity or
a public packaged release. B50 still owns broad parity expansion, installers,
publication/provenance, and performance work. Its minimum command-compatibility
and native execution checks cannot be postponed for commands shipped in B40.

### Assessment and gaps

Assessed on 2026-11-03 against the current working tree and the
[latest remote CI run](https://github.com/MauriceKremer/sfclirust/actions/runs/41482826007).
The remote run covers `c909353a55fe3e0cefa67dbcf6f8dc45e4e49bae`, not the later
local commit or uncommitted B20/B30/B40 implementation.

| Area | Evidence | Work needed before B40 closure |
|---|---|---|
| Authentication | B20-02 has an in-memory backend, not OS persistence. B20-03 has OAuth/session primitives, not browser launch, identity lookup, or complete commands. | B20-04 and B20-05. A login must survive process exit without plaintext credentials. |
| Executable integration | The parser exposes only `org list`, which returns unsupported. OS signal registration is not connected in `main`; output helpers do not establish real command JSON contracts. | Wire and test the production entry point in B20-05, then reuse it for query and metadata commands. |
| Authenticated query | B30-02 supplies REST/query primitives; `data query` remains planned. | B30-05, including executable and pinned compatibility tests. |
| Metadata | B40-01/02 supply project loading, ApexClass/ApexTrigger mapping, and ZIP safety. No deploy/retrieve command is supported. | B40-03/04, including concrete command contracts, job recovery, and safe materialization. |
| CI | Remote Audit failed to resolve an action; Deny used cargo-deny 0.14.21 and failed to parse the advisory database. The audit action was replaced locally, but has no green remote run. Current strict Clippy fails on three `collapsible_if` diagnostics in `config/store.rs`. | B00-06; verify the current revision, not an older run or a lint-suppressed invocation. |
| Enforcement and evidence | `main` now requires strict `Required CI`, including for administrators. Coverage/MSRV and the changed workflow still have no passing remote run on this uncommitted tree. | B00-06 awaits remote cross-platform evidence; B40-05 repeats all gates on the final revision. |

### Execution order

Implement one test-first vertical slice at a time. Keep existing task IDs; the
new tasks close specific gaps rather than restarting completed library work.

| Step | Task | Prerequisites | Deliverable |
|---|---|---|---|
| 1 | [B00-06: restore and enforce green CI](00-foundation.md#b00-06-restore-and-enforce-the-ci-baseline) | Existing workspace and CI configuration | Aligned pinned tools, passing strict checks/coverage/MSRV/native jobs, and required merge checks. |
| 2 | [B20-04: persistent native auth state](20-state-auth.md#b20-04-implement-persistent-native-auth-state) | B20-01/02/03 primitives, B00-06 | OS secure-store adapters, non-secret org index, cross-process coordination, and explicit ephemeral mode. |
| 3 | [B20-05: runnable auth and org commands](20-state-auth.md#b20-05-deliver-runnable-authentication-and-org-commands) | B20-04, B10 foundations, B30-01 transport | Browser login, identity resolution, list/display/logout, alias/default selection, and real CLI lifecycle/output wiring. |
| 4 | [B30-05: runnable query](30-salesforce-api.md#b30-05-deliver-the-authenticated-query-command) | B20-05, B30-01/02/03 | `data query` through the same production auth/runtime path. |
| 5 | [B40-03: deployment lifecycle](40-project-metadata.md#b40-03-implement-deployment-submission-and-status-lifecycle) | B20-05, B30-01/03/04, B40-01/02 | Selected deploy/validate and report/resume contracts, bounded waiting, recoverable job IDs. |
| 6 | [B40-04: retrieval/materialization](40-project-metadata.md#b40-04-implement-retrieval-and-filesystem-materialization) | B40-03 integration, B40-01/02, B20-05, B30-04 | Selected retrieve lifecycle, reviewed overwrite policy, and safe/recoverable writes. |
| 7 | [B40-05: review and readiness gate](40-project-metadata.md#b40-05-review-and-accept-the-runnable-b40-milestone) | All preceding deliverables, including B30-05 | Reviewed fixes, repeatable user walkthrough, native/live evidence, and green required CI on the exact delivered revision. |

B30-05 and B40-03 can proceed independently once their prerequisites pass;
proving the simpler query first is the recommended integration order.
Do not start B50 expansion while B40-05 remains open.

### Scope boundaries and readiness evidence

- Initial user commands: `org login web`, `org list`, `org display`, `data query`,
  `org logout`, and the selected deploy/retrieve lifecycle commands. Document
  supported flags and intentional differences before implementing each one.
- Support login-time alias/default settings and explicit target selection.
  Standalone alias/config management commands and access-token-login parity are
  deferred unless separately inventoried. Ephemeral token injection is required
  for authenticated operations in headless mode; it is not a persistent login
  performed by an earlier process.
- Keep metadata limited to ApexClass/ApexTrigger with required sidecars and
  concrete members. Wildcards, additional types, and general source conversion
  remain explicitly unsupported.
- Ordinary tests use temporary native state, synthetic data, loopback servers,
  and in-memory credentials. Real secure-store tests run only in explicitly
  selected, disposable platform environments; live tests use authorized disposable
  Salesforce orgs. Neither may touch the developer's existing auth state.
- The final walkthrough must use the built executable across separate invocations:
  login, list/display, query, validate/deploy, report/resume, retrieve, logout.
  Verify credential removal, failure paths, and recovery as well as success.
- Missing live-test credentials, a missing native platform result, or insufficient
  repository permissions is an explicit blocker, not a passing skip. Record the
  required external action and leave the gate open.

## Completion rule

Check a task only after its files exist, acceptance criteria pass, tests are
committed, and relevant documentation is updated. If implementation reveals a
contradiction in the design, amend the design and this backlog in the same
change. Record intentional compatibility differences rather than silently
changing the contract.

For B40 closure, attach the command/test evidence and successful CI run to the
same committed revision. A local pass, a completed library task, or a review with
unresolved milestone-blocking findings is not sufficient.
