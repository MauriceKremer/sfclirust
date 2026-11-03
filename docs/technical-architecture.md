# Native Salesforce CLI: Architecture and Delivery Plan

**Project:** `sf-rust`, an independent Rust-native reimplementation of Salesforce
CLI behavior based on the open-source [Salesforce CLI](https://github.com/salesforcecli/cli).

**Status:** Design specification. This workspace currently contains documentation,
not an implemented CLI, Cargo workspace, or enforcing CI pipeline.

**Confirmed scope:** Native-only runtime with phased delivery toward first-party
command parity. No Node.js bridge, automatic fallback to `sf`, or execution of
legacy JavaScript plugins.

## 1. Goals and Explicit Limits

Build a maintainable native executable that preserves supported Salesforce CLI
workflows, particularly script-facing behavior, while improving startup cost,
distribution simplicity, and predictable resource use. This is a behavioral
reimplementation, not a mechanical translation of TypeScript files into Rust.

- Ship working vertical slices, not a command tree full of success-shaped stubs.
- Define parity against a pinned upstream baseline, not a moving `latest`.
- Preserve supported command paths/aliases, flags, JSON contracts, exit behavior,
  configuration semantics, and Salesforce project formats through tests.
- Preserve safety and correctness before optimizing. Performance claims require
  measurements on equivalent work.
- Treat intentional differences as public compatibility decisions, with tests
  and release notes, rather than silently calling them parity.

First-party parity is the long-term direction, not a promise that every upstream
command ships in the initial release. Legacy npm plugin loading, the `oclif`
extension interface, automatic self-update, and all historical `sfdx` aliases are
not implicit requirements of the first release. Third-party plugins are outside
the first-party parity promise.

## 2. Upstream Baseline and Porting Method

The official CLI repository is the entry point, not the complete implementation
of every workflow. A port must trace a command to its owning plugin and the
libraries implementing its behavior, rather than mapping only the top-level
command name.

### Verified source map

The upstream [architecture description](https://github.com/salesforcecli/cli/blob/8ca3ae36d11a01726aec741897547adec3aa5ad2/ARCHITECTURE.md)
and [host manifest](https://github.com/salesforcecli/cli/blob/8ca3ae36d11a01726aec741897547adec3aa5ad2/package.json)
show an `oclif` host with framework plugins, bundled Salesforce plugins,
just-in-time (JIT) npm plugins, and lifecycle hooks. Command behavior also depends
on shared libraries; porting the host alone does not port Salesforce CLI.

These are source-discovery snapshots inspected on **2026-11-02**, not a proven
set of dependencies installed together in one published distribution:

| Upstream source | Inspected revision | What to trace |
|---|---|---|
| `salesforcecli/cli` | [`8ca3ae36d1`](https://github.com/salesforcecli/cli/tree/8ca3ae36d11a01726aec741897547adec3aa5ad2), manifest version `2.158.4` | Packaging, plugin inventory, topic parsing, hooks, and parser exit-code configuration. |
| `salesforcecli/sf-plugins-core` | [`bc6bc6f14a`](https://github.com/salesforcecli/sf-plugins-core/tree/bc6bc6f14add235cdc739a38eb244c404c2d587c) | `SfCommand`, JSON/error rendering, signals, and project/org requirements. |
| `forcedotcom/sfdx-core` (`@salesforce/core`) | [`23d7f942fa`](https://github.com/forcedotcom/sfdx-core/tree/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b) | Config precedence, authentication, org/project models, [project resolution](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/sfProject.ts), [project field schema](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/schema/sfdx-project/sfdxProjectJson.ts), encryption and state handling. |
| `salesforcecli/plugin-deploy-retrieve` | [`2e867d60f0`](https://github.com/salesforcecli/plugin-deploy-retrieve/tree/2e867d60f01a148fac1cbaf469136fed3314eaf3) | Deploy/retrieve command flags, validation, and workflow orchestration. |
| `forcedotcom/source-deploy-retrieve` | [`b8998b38cf`](https://github.com/forcedotcom/source-deploy-retrieve/tree/b8998b38cf5b4d3278f5fb1f964b96d5a47886d7) | Source/metadata conversion and deployment/retrieval implementation. |

For example, [`project deploy start`](https://github.com/salesforcecli/plugin-deploy-retrieve/blob/2e867d60f01a148fac1cbaf469136fed3314eaf3/src/commands/project/deploy/start.ts)
has mutually exclusive selectors, dependent flags, typed wait durations, and
async behavior. Its command path alone is insufficient to define compatibility.
Trace other command families to their own plugin sources before specifying them;
this snapshot is not an exhaustive audit of every command or project schema.

### Required executable baseline

Before implementation, record a reproducible baseline containing:

1. A selected published CLI release and its immutable source commit.
2. The exact installed/default plugin versions and their source revisions.
3. The distribution/package lock and integrity information needed to reconstruct
   the reference executable, including its Node.js version for test use.
4. The Salesforce API versions used by each supported workflow.
5. A machine-readable command inventory as defined by the
   [testing strategy](testing-strategy.md).

The baseline is a development/test dependency only. Installing or running the
native application must not install Node.js or the reference CLI.

For each command: locate upstream ownership, inspect success and failure
behavior, capture sanitized contract fixtures, implement the native workflow,
and compare it with the pinned oracle. Upstream branch URLs are discovery links,
not immutable compatibility targets. Baseline upgrades need an explicit inventory
diff and regression tests.

## 3. Compatibility Contract

| Surface | Required treatment |
|---|---|
| Commands | Implement the supported path, aliases, arguments, flags, defaults, and validation rules. Unsupported commands fail clearly with a nonzero exit; never silently invoke another executable. |
| Machine output | Match the inventoried JSON shape, field types, null/absent distinctions, error/warning behavior, and exit status. Keep diagnostics and progress off stdout. |
| Human output | Preserve useful information and workflow semantics. Exact wording/layout is promised only when recorded in a contract; document other differences. |
| Interaction | Test TTY/non-TTY behavior, confirmations, stdin, interruption, and broken pipes. Do not hang waiting for input in automation. |
| Projects | Read supported `sfdx-project.json`, package directories, manifests, and metadata layouts without converting the project to a proprietary format. Reject unsupported cases explicitly. |
| Configuration | Preserve the supported command's flag/environment/project/user precedence and alias/default-org meaning. Native state ownership is an intentional difference described below. |
| Authentication | Support explicitly shipped OAuth/token flows and their lifecycle. No automatic reuse or rewriting of another CLI's encrypted credential store. |
| API behavior | Honor the workflow's selected API version, pagination, limits, async job status, and error semantics. Do not blindly choose the newest server version. |
| Plugins | Existing JavaScript/TypeScript `oclif` plugins do not execute in the native-only distribution. A future native protocol is a new extension model, not legacy plugin compatibility. |

`clap` is a parsing tool, not proof of `oclif` compatibility. Its default help,
parsing errors, exit codes, and output streams must be adapted where the supported
contract differs. Preserve command-specific formats such as CSV only where the
command actually supports them; do not invent a universal output-format flag.

In the inspected [`SfCommand`](https://github.com/salesforcecli/sf-plugins-core/blob/bc6bc6f14add235cdc739a38eb244c404c2d587c/src/sfCommand.ts),
JSON mode can also be selected by `SF_CONTENT_TYPE=JSON` for participating
commands. JSON errors are emitted as structured stdout results with a nonzero
process exit, not merely as human-readable stderr. Test these paths explicitly.
Do not reproduce JavaScript stack traces or runtime-specific exceptions as a Rust
implementation detail; record any resulting contract differences.

## 4. Modules and Dependency Direction

Start with three crates:

```text
sf-cli  -->  sf-core  -->  sf-api
   |            |             |
   |            |             +-- HTTP/SOAP, wire models, pagination
   |            +-- workflow state, auth, config, project interpretation
   +-- arguments, terminal interaction, JSON/human rendering, exit mapping
```

### `sf-cli`: executable interface

Owns the `sf-rust` binary, command dispatch, a single async runtime, signals,
stdin/stdout/stderr, and rendering. It translates parsed input into typed requests
and typed outcomes into the required output/exit behavior. Workflow code does not
print directly or terminate the process.

Keep CLI-facing output models separate from Salesforce wire structs where the
formats differ. A server schema change must not accidentally change public JSON.
An error renderer must handle parse failures as deliberately as network failures.

### `sf-core`: workflow implementation

Owns authentication, org selection, configuration precedence, project loading,
and command workflows. Its interfaces should hide the sequencing needed to
complete an operation, not expose dozens of low-level calls to every command.

Use focused internal modules and typed request/result/error values. Configuration
storage, credential storage, clock, and transport need replaceable adapters only
where behavior genuinely varies, such as OS storage versus ephemeral/test storage.
Avoid a trait hierarchy mirroring the upstream TypeScript class tree.

### `sf-api`: Salesforce transport implementation

Owns reusable HTTP clients, REST/Tooling requests, Metadata SOAP, wire types,
pagination, and transport-level failure classification. It does not depend on
CLI parsing, print output, choose the default org, or read the user's home.

Accept an authenticated session and transport settings through its interface;
`sf-core` owns credential refresh and workflow policy. Keep retry decisions tied
to operation semantics: a generic HTTP retry wrapper must not replay arbitrary
deployments or other mutations.

Use library-backed XML serialization/parsing, namespace handling, fault detection,
and archive handling. "SOAP support" is not just posting an XML string; metadata
conversion, deployment job lifecycle, and retrieval materialization need explicit
workflow implementations and fixtures.

Public interfaces should be small and deep: callers exercise meaningful behavior
without reproducing implementation details. Tests cross those same interfaces.
Do not add universal utility/type crates, dynamic libraries, or a plugin framework
until real reuse or extension requirements justify them.

## 5. State, Authentication, and Project Interoperability

Use a dedicated `sf-rust` namespace under each OS's per-user application data
directory, with an explicit `SF_RUST_STATE_DIR` override for automation and tests.
Resolve that directory through a platform-aware library. Project-local native
preferences belong in a separate `.sf-rust` directory, which must be ignored by
version control once the scaffold is added.

Non-secret config and credentials have separate interfaces and lifecycles.
Credentials use the OS secure store for interactive use or a documented ephemeral
headless mode. Do not require a Linux desktop keyring for an explicitly ephemeral
CI workflow, and do not silently replace unavailable secure storage with plaintext.

Existing Salesforce project files remain the interoperability surface. The loader
must preserve relevant unknown fields and avoid rewriting unrelated project data.
Non-secret upstream preferences/aliases may be read through an explicit
compatibility/import operation; native writes stay in native state. Do not infer
that a similar-looking auth JSON file is safe to read, decrypt, or mutate.

This state isolation is an intentional difference, not transparent shared-login
compatibility. Document which defaults/config keys are supported and how users
authenticate separately. Credential import or shared writable state is deferred
until its format, encryption, concurrency, and migration behavior are verified.

The [security specification](security-compliance.md) owns secret handling,
permissions, OAuth validation, safe writes, TLS, and untrusted input requirements.

## 6. Workflow and Resource Behavior

A typical invocation follows:

```text
parse -> validate context -> resolve org/config -> obtain credentials
      -> execute workflow -> classify outcome -> render -> choose exit status
```

Configuration and credentials are loaded only when the command needs them:
`--help`, `--version`, and appropriate local commands must not require network
access or a working credential store.

Async work has deadlines, bounded concurrency, and tracked task lifetimes. Stream
large records/archives where the command's output contract permits; where JSON
requires a complete envelope, define bounded buffering/spooling and failure
behavior before emitting partial output. Never silently truncate results to meet
a resource limit.

Deploy/retrieve and other long-running workflows must distinguish submission,
remote job identity, polling, success, partial failure, timeout, and interruption.
Preserve the job ID for follow-up reporting. Cancelling a local wait does not mean
the remote operation was cancelled. No timeout or unknown remote state is success.

## 7. Extension Policy

First-party commands are compiled into the binary. The original "binary-subprocess"
proposal is not a way to run existing `oclif` plugins unchanged.

The upstream host's [JIT plugin and hook declarations](https://github.com/salesforcecli/cli/blob/8ca3ae36d11a01726aec741897547adec3aa5ad2/package.json)
are Node/npm extension mechanisms. JIT installation, npm `plugins install/link`,
and JavaScript lifecycle hooks have no planned runtime equivalent here. Inventory
their first-party command workflows separately: a workflow may be ported natively
without reproducing the original installation mechanism. Workflows inherently
requiring a JavaScript runtime remain explicitly deferred or unsupported until an
approved native replacement exists.

Defer external plugins until core workflows are reliable. If later needed, define
a versioned subprocess protocol, explicit trusted registration, compatibility
negotiation, structured errors, cancellation, credential scope, and output limits.
JSON-RPC may be evaluated then; it is not an implemented or required MVP mechanism.
Do not introduce a Rust dynamic-library ABI or automatic untrusted `PATH` discovery.

## 8. Phased Delivery and Exit Criteria

| Phase | Scope | Exit criteria |
|---|---|---|
| M0: foundation | Cargo workspace, toolchain/MSRV, lints, CI, binary identity, baseline inventory, config/credential test adapters, output/error contracts. | The checks in the [development guide](development-setup.md) are implemented and required; an offline executable smoke test passes on supported OSs; no Salesforce credentials are needed for ordinary tests. |
| M1: authenticated read workflows | Small coherent slice: browser and ephemeral token login, logout, supported alias/config operations, org list/display, and SOQL query. | Every shipped command has positive/negative compatibility tests; JSON/exit behavior and credential lifecycle are verified; a disposable-org test proves the end-to-end slice. Secrets displayed by a command are explicitly classified. |
| M2: project and metadata workflows | Project/package/manifest interpretation, source conversion, deploy/validate/report/retrieve, and their async lifecycle, added incrementally. | Synthetic project/metadata fixtures and live test orgs cover supported types, partial failures, recovery, cancellation, and safe file writes. Unsupported metadata types are explicit. |
| M3: expand first-party parity | Prioritized org provisioning, Apex/testing, data/Bulk, packaging, and remaining inventoried first-party commands. | Each command graduates separately through the same compatibility and security gates; progress is measured against the pinned inventory, not the number of registered names. |

The M1 list is the starting implementation backlog, not a claim that those
commands are already available. Restrict its first release further if needed,
but never omit a workflow's failure handling or credential cleanup to call it done.

### B40 developer-preview checkpoint

The [B40 delivery plan](backlog/README.md#b40-delivery-target-runnable-developer-preview)
combines the initial M1 user journey with the narrow M2 metadata slice. It requires
browser login with durable OS credentials, explicit ephemeral authentication,
org list/display/logout, SOQL query, and supported ApexClass/ApexTrigger
deploy/retrieve workflows through the real executable. Alias/default settings
are supported through login and target selection; standalone alias/config
management and access-token-login command parity remain deferred.

Close the missing secure-store and executable integration work before treating
metadata commands as usable. After B40-03/04, B40-05 reviews and stabilizes the
combined implementation and requires green CI plus native/live evidence on the
delivered revision. This checkpoint is a runnable build-from-source preview;
public packaging and broader parity remain B50 work, without weakening the
required command compatibility or security gates.

Each milestone must include documentation, tests, and recoverable error behavior.
No release may advertise blanket Salesforce CLI compatibility while unsupported
commands or intentional differences remain undisclosed.

## 9. Document Ownership and Acceptance

- [Implementation backlog](backlog/README.md): ordered tasks, dependencies,
  acceptance criteria, and likely source-file ownership.
- [Development and setup](development-setup.md): workspace, Rust idioms, dependency
  choices, lint inheritance, toolchain, and required local commands.
- [Testing and release gates](testing-strategy.md): command inventory, differential
  comparisons, coverage, platform matrix, live tests, and release evidence.
- [Security and compliance](security-compliance.md): credentials, trust, files,
  network behavior, dependency policy, and distribution obligations.

These are normative requirements for future changes. They do not replace Cargo
configuration, a CI workflow, repository rules, or executable compatibility tests.
