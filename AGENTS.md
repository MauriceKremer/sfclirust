# AGENTS.md

`sf-rust` is a native Rust reimplementation of Salesforce CLI (`sf`), delivered
in vertical slices toward first-party command parity with a pinned upstream
baseline. The shipped binary is native-only: Node.js exists solely to run the
upstream oracle inside compatibility tests.

**Current state:** specs and backlog only. No Cargo workspace, CI, or
compatibility inventory exists yet, so every check the docs describe is a
requirement to build, not a gate that already runs.

## Workflow

1. **Pick** the lowest-numbered unchecked task in the
   [backlog](docs/backlog/README.md) whose dependencies are complete (first:
   `B00-01`). Done when you can state its ID, acceptance criteria, and files.
2. **Read** every doc that governs the task's files (see *Where to look*). Done
   when each requirement those docs place on the task appears in your plan.
3. **Inventory** before porting any command: add its entry to
   `docs/compatibility/commands.yaml` (created by `B00-01`) as specified in
   [testing section 1](docs/testing-strategy.md#1-define-compatibility-before-implementing-a-command).
4. **Implement** test-first through the module's public interface: a failing
   test at the interface, then the code that turns it green.
5. **Verify** with the
   [required developer checks](docs/development-setup.md#6-setup-and-required-developer-checks)
   once the workspace exists. Done when all pass and every acceptance criterion
   is demonstrated by a committed test.
6. **Close** in the same change: tick the task's checkboxes, update any doc
   whose design the work changed, and record intentional compatibility
   differences in the inventory.

## Where to look

| Trigger | Source of truth |
|---|---|
| Choosing, scoping, or completing work | [Backlog README](docs/backlog/README.md), then the work-package file |
| Crate layout, dependencies, toolchain, lints, Rust idioms, local checks | [development-setup.md](docs/development-setup.md) |
| Command scope, module ownership, state layout, milestones | [technical-architecture.md](docs/technical-architecture.md) |
| Porting upstream behaviour | [Verified source map](docs/technical-architecture.md#verified-source-map) and the [compatibility contract](docs/technical-architecture.md#3-compatibility-contract) |
| Tests, fixtures, differential comparisons, CI, coverage, release gates | [testing-strategy.md](docs/testing-strategy.md) |
| Credentials, state files, HTTP/TLS, XML, archives, subprocesses, licensing | [security-compliance.md](docs/security-compliance.md) |

The docs are normative; this file only routes to them. When code and a doc
disagree, fix whichever is wrong in the same change.

## Guardrails

These protect real user state and the compatibility promise before lints and CI
exist to catch them:

- Run the development binary as `sf-rust`. Reach the reference CLI only through
  the pinned oracle path in compatibility tests, never a bare `sf` on `PATH`.
- Point tests at temporary state (`SF_RUST_STATE_DIR`) and an in-memory
  credential store, leaving the developer's `~/.sf`, `~/.sfdx`, and OS keychain
  untouched. Live orgs appear only in opt-in suites using disposable orgs.
- Register a command only once its workflow runs end to end; every other
  command returns a clear nonzero "unsupported" error.
- Persist secrets only in the OS secure store or the ephemeral headless mode.
  When storage is unavailable, return an error.
- Cite upstream code by the immutable commit SHAs in the source map, port
  behaviour rather than TypeScript structure, and keep required attribution for
  adapted material.
