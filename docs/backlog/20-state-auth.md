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

**Status: Library foundation implemented; production persistence remains B20-04.**

- [x] Define a narrow credential-store interface and an in-memory test
  implementation. Implement actual OS secure-store adapters in B20-04.
- [x] Add typed secret wrappers and ensure `Debug`, serialization, tracing, and
  error contexts cannot expose contents.
- [x] Define an in-memory credential source suitable for ephemeral use. Actual
  headless input/command wiring remains B20-05; it must not persist tokens or
  pass them in child-process arguments.
- [x] Fail clearly if secure storage is unavailable. Do not add a plaintext or
  generic-keychain fallback without a separately approved design.

**Implementation:** [B20-02-IMPLEMENTATION.md](B20-02-IMPLEMENTATION.md)

**Summary:**
- 15 tests passing (credential store + secret safety)
- CredentialBackend interface with InMemoryStore implementation
- Secret<T> wrapper hides contents in Debug/Display/errors
- Ephemeral headless mode for CI/CD
- LogoutAction enum distinguishes local deletion from remote revocation
- No OS secure store yet; platform adapters are required by B20-04 before B40 closure

**Files:**
- `crates/sf-core/src/auth/mod.rs` — module exports
- `crates/sf-core/src/auth/secret.rs` — typed secret wrapper
- `crates/sf-core/src/auth/credentials.rs` — credential types and namespacing
- `crates/sf-core/src/auth/store.rs` — store interface and InMemoryStore
- `crates/sf-core/tests/credential_store.rs` — comprehensive tests

## B20-03: Implement org selection and auth lifecycle

**Status: Library slice implemented** — Auth/session primitives and org resolution are
implemented and tested. CLI commands remain `planned` in the compatibility
inventory until B20-04/05 provide secure persistence and verified command-level
contracts. This is not completion of the original user-facing auth acceptance.

- [x] Inventory first-release org/login/logout candidates. Reconcile the exact
  supported flag, output, error, and side-effect contracts in B20-05.
- [x] Implement only selected OAuth flows, including state validation, PKCE
  where supported, loopback-only callback, timeout, and cancellation cleanup.
- [x] Implement token refresh concurrency, expiry, revocation, and failure
  behavior with no unbounded retry or reauthentication loop.
- [x] Resolve username/alias/default org with tested precedence and clear errors.

**Implementation:** [B20-03-IMPLEMENTATION.md](B20-03-IMPLEMENTATION.md)

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
- The remaining live-login and supported-command JSON/exit acceptance is owned
  by B20-05 and required for B40-05, not satisfied by these library tests.

## B20-04: Implement persistent native auth state

**Depends on:** B00-06 and the B20-01/02/03 library interfaces.

- [ ] Implement the actual `keyring` backends for macOS Keychain, Windows
  Credential Manager, and a documented Linux secret store, including explicit
  platform features/prerequisites. Missing or locked storage is an actionable
  error, never an empty successful list or a plaintext fallback.
- [ ] Persist only non-secret org identity, aliases/defaults, and secure-store
  references in native state. Define stable account/org keys and a listing index;
  do not assume the OS store can enumerate every credential.
- [ ] Define coordinated credential/index updates, rollback or reconciliation
  after partial writes, atomic state replacement, restrictive permissions, and
  deletion of credentials plus stale aliases/defaults.
- [ ] Coordinate refresh/update/logout across separate CLI processes, not only
  in-process tasks. Re-read credentials after locking and prevent a concurrent
  refresh from resurrecting credentials after logout.
- [ ] Preserve an explicitly selected in-memory backend for tests and ephemeral
  headless operations. It must not masquerade as durable interactive login.

**Likely files**

- `crates/sf-core/src/auth/{store,credentials,session}.rs`
- `crates/sf-core/src/auth/` (platform adapters and native org index)
- `crates/sf-core/src/config/`
- `crates/sf-core/tests/credential_store.rs`
- `crates/sf-core/tests/auth_lifecycle.rs`
- `Cargo.toml`, `crates/sf-core/Cargo.toml`, `Cargo.lock`

**Acceptance**

- Public store/session tests cover restart, multiple orgs/aliases, concurrent
  processes, refresh/logout races, corrupt index, permission errors, unavailable
  store, partial update recovery, and deletion. No token appears in native files.
- Default tests use fake/in-memory storage and temporary state only. Explicit
  native adapter tests use disposable runner accounts/stores with a unique
  namespace and cleanup, never the developer's OS keychain.
- Selected native backend features build and run on every advertised platform.
  Missing platform evidence blocks readiness rather than silently using a mock.

## B20-05: Deliver runnable authentication and org commands

**Depends on:** B20-04, B10's parser/output/runtime foundations, and B30-01.
This task owns the production CLI integration still missing from those foundations.

- [ ] Recheck the selected `org login web`, `org list`, `org display`, and
  `org logout` contracts against the pinned upstream source/oracle before coding.
  Define supported flags, aliases, defaults, JSON/exit behavior, prompts, and
  intentional native limitations. Resolve secret-bearing display output
  explicitly; do not claim parity by silently redacting required fields.
- [ ] Wire browser launch/manual authorization instructions, OAuth client
  configuration, callback/token exchange, identity lookup, and secure persistence
  into a complete login. Document how to provision the explicit native client ID,
  approved scopes, and compatible loopback callback settings before live testing.
- [ ] Make login-time alias/default settings persist, then resolve the same org
  in a later process by explicit target or configured default. Implement real
  list/display and logout with accurate local-deletion/revocation outcomes.
- [ ] Wire explicit ephemeral token injection into authenticated operations via
  documented stdin/environment input. Define org/instance selection and reject
  incompatible persistence options. A previous ephemeral login process cannot
  supply credentials to a later process; `org login access-token` parity remains
  deferred unless separately completed.
- [ ] Connect the actual binary's command dispatch, lazy state/client creation,
  `--json` and `SF_CONTENT_TYPE=JSON`, diagnostics, prompts, output flush, and OS
  signals to the workflow cancellation path. Propagate output/storage failures,
  join spawned tasks, and keep `--help`/`--version` offline.
- [ ] Add the minimum pinned offline differential tests or reviewed
  oracle-derived fixtures now, rather than deferring all compatibility evidence
  to B50-02. Record unsupported variants; register only complete handlers.

**Likely files**

- `crates/sf-cli/src/{main,cli,output,runtime}.rs`
- `crates/sf-cli/src/commands/org.rs` and command modules as needed
- `crates/sf-core/src/auth/`
- `crates/sf-cli/tests/` (auth commands, output, process lifecycle)
- `docs/compatibility/commands.yaml` and reviewed fixtures
- `README.md` (working native login/headless instructions)

**Acceptance**

- Executable tests exercise login -> new-process list/display -> logout and
  missing/wrong org, denied/cancelled/expired login, refresh failure, failed
  storage, non-TTY input, JSON errors, broken pipe, and real process interruption.
  Use temporary state and synthetic test adapters; helper-only tests do not count
  as evidence for command wiring.
- One opt-in disposable-org run proves browser login, identity, persistence
  across invocations, authenticated access, and cleanup. OAuth client provisioning
  or unavailable credentials is a visible blocker, not a success-shaped skip.
- No secrets appear in arguments, logs, snapshots, or unrelated command output.
  Headless commands persist no tokens and do not require an OS secure store.
- Commands become `supported` only after their required offline, differential,
  negative, native-store, and live evidence passes. B40-05 rechecks the combined
  user journey on the final revision.
