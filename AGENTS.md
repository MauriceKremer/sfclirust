# AGENTS.md

## ⚠️ CORE PRINCIPLE: NO MISTAKES. NO CI MISTAKES.

`sf-rust` is a production-grade native Rust reimplementation of Salesforce CLI (`sf`),
delivered in vertical slices toward first-party command parity with a pinned upstream
baseline. The shipped binary is native-only: Node.js exists solely to run the
upstream oracle inside compatibility tests.

**Every change must be correct, complete, and pass all CI checks before commit.**

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

## Code Quality & CI Requirements

**Before committing ANY code, verify all checks pass:**

```bash
cargo fmt --check       # ✅ Format must be correct
cargo build --all       # ✅ All crates compile
cargo clippy --all-targets  # ✅ Zero clippy warnings/errors
cargo test --all        # ✅ All tests pass, zero warnings
cargo deny check         # ✅ Licenses, security, dependencies ok
cargo audit             # ✅ No known vulnerabilities
```

**Pre-commit verification checklist:**
- [ ] Code compiles cleanly without warnings
- [ ] All tests pass with zero warnings
- [ ] Clippy reports zero errors
- [ ] Code is properly formatted
- [ ] License and security checks pass
- [ ] All acceptance criteria have test coverage
- [ ] Implementation documented (IMPLEMENTATION.md)
- [ ] Backlog item status updated
- [ ] Commit message is detailed and references the backlog item

**Common pitfalls to avoid:**
- ❌ Don't use `unwrap()` or `expect()` in production code (workspace forbids)
- ❌ Don't use `unsafe` blocks (workspace forbids)
- ❌ Don't touch user's `~/.sf`, `~/.sfdx`, or OS keychain
- ❌ Don't skip CI checks or commit with warnings
- ❌ Don't accept malformed files silently
- ❌ Don't leave code partially working
- ✅ Use temp directories (`TempDir`) for test state
- ✅ Make errors explicit with Result<T, E>
- ✅ Test happy path, edge cases, and error paths
- ✅ Write tests before code (TDD)
- ✅ Add `#![allow(clippy::expect_used, clippy::unwrap_used)]` to test-only files

## Acceptance Criteria Verification

For each backlog item, verify **all** acceptance criteria:
1. Read every requirement in the backlog document
2. Each requirement has at least one test demonstrating it
3. All tests pass with zero warnings
4. No system state touched (only temp dirs and mocks)
5. Errors are explicit and tested
6. Implementation documented
7. All CI checks pass
8. Commit message references the backlog item ID
