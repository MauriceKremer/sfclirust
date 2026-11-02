# 2. Configuration, State, and Authentication

**Goal:** provide isolated native state and explicit authentication workflows.

**Depends on:** workspace foundation and CLI's typed outcomes. API calls depend
on the authenticated-session interface defined here.

## B20-01: Add platform-aware state paths and isolated configuration

**Status: ✅ COMPLETE**

- [x] Define native global and project-local state paths. Support
  `SF_RUST_STATE_DIR` for tests/automation.
- [x] Parse supported non-secret preferences with explicit defaults and
  precedence: command flag, environment, project, native user state.
- [x] Read/import upstream non-secret settings only through a documented
  compatibility operation; never rewrite upstream files.
- [x] Make config file writes atomic where supported, permission-aware, and
  concurrency-safe. Preserve unknown project fields if updating project files.

**Implementation:** [B20-01-IMPLEMENTATION.md](B20-01-IMPLEMENTATION.md)

**Summary:**
- 38 tests passing (18 unit + 20 integration)
- 5-level precedence: CLI > Env > Project > Global > Default
- Platform-aware paths: macOS/Linux/Windows support
- Atomic writes with corruption protection
- Thread-safe concurrent access
- Symlink and path behavior deliberate and tested
- No system state touched; fully isolated to temp dirs in tests

**Files:**
- `crates/sf-core/src/config/mod.rs` — module exports
- `crates/sf-core/src/config/paths.rs` — platform-aware state paths
- `crates/sf-core/src/config/store.rs` — precedence-based config store
- `crates/sf-core/tests/config_precedence.rs` — comprehensive tests

## B20-02: Define credential storage and secret types

- [ ] Define a narrow credential-store interface and adapters for supported OS
  secure stores plus an in-memory test implementation.
- [ ] Add typed secret wrappers and ensure `Debug`, serialization, tracing, and
  error contexts cannot expose contents.
- [ ] Define an ephemeral headless credential source. It must not persist tokens
  or pass them in child-process arguments.
- [ ] Fail clearly if secure storage is unavailable. Do not add a plaintext or
  generic-keychain fallback without a separately approved design.

**Likely files**

- `crates/sf-core/src/auth/mod.rs`
- `crates/sf-core/src/auth/credentials.rs`
- `crates/sf-core/src/auth/secret.rs`
- `crates/sf-core/tests/credential_store.rs`

**Acceptance**

- Tests prove namespacing, persistence lifecycle, unavailable-store behavior,
  and no secret leakage.
- Platform backend behavior is tested natively before declaring an OS supported.
- Logout distinguishes local deletion from remote revocation.

## B20-03: Implement org selection and auth lifecycle

- [ ] Inventory first-release org/login/logout commands and exact contract.
- [ ] Implement only selected OAuth flows, including state validation, PKCE
  where supported, loopback-only callback, timeout, and cancellation cleanup.
- [ ] Implement token refresh concurrency, expiry, revocation, and failure
  behavior with no unbounded retry or reauthentication loop.
- [ ] Resolve username/alias/default org with tested precedence and clear errors.

**Likely files**

- `crates/sf-core/src/auth/oauth.rs`
- `crates/sf-core/src/auth/session.rs`
- `crates/sf-core/src/orgs/mod.rs`
- `crates/sf-core/tests/auth_lifecycle.rs`
- `crates/sf-cli/src/commands/auth/`
- `crates/sf-cli/src/commands/org/`

**Acceptance**

- Synthetic tests cover refresh success/failure, revocation, concurrent refresh,
  cancellation, no keychain, and wrong-org selection.
- Live login is opt-in and tested only with a disposable least-privilege org.
- Supported commands match their pinned JSON/exit contracts.
