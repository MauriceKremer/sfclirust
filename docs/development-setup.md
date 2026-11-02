# Rust Development and Setup Requirements

**Status:** Implementation specification; the workspace and checks described here
do not exist yet. They are required deliverables for the bootstrap milestone, not
claims about current enforcement.

See [architecture and delivery scope](technical-architecture.md),
[testing and CI](testing-strategy.md), and
[security requirements](security-compliance.md). MUST denotes a release or merge
requirement; SHOULD allows a documented, reviewed exception.

## 1. Toolchain and Supported Platforms

- Use stable Rust, Edition 2024, and Cargo workspace resolver `"3"`. Nightly is
  permitted only for isolated development tools, not for shipping the application.
- The bootstrap change MUST pin an exact stable release in `rust-toolchain.toml`,
  including `rustfmt` and `clippy`. "Latest stable" is an update policy, not a pin.
- Declare `rust-version` in `[workspace.package]` and inherit it in every member.
  This is the minimum supported Rust version (MSRV), which may differ from the
  development toolchain. Select it against the actual dependency graph and verify
  it in CI; the edition's minimum compiler alone is not evidence of compatibility.
- Commit the application workspace's `Cargo.lock`. CI and release builds MUST use
  `--locked`; lockfile changes require review. Keep dependencies and shared package
  metadata in the root manifest and inherit them where appropriate.
- The initial release matrix is Linux `x86_64-unknown-linux-gnu`, macOS
  `aarch64-apple-darwin` and `x86_64-apple-darwin`, and Windows
  `x86_64-pc-windows-msvc`. Each needs native execution tests, not only a
  cross-compilation result. Document the minimum OS/libc versions when packaging.
- Native Rust does not mean a universally static executable. Keychain access,
  certificate trust, and platform libraries must work on the supported systems.
  Additional architectures require their own tested release artifacts.

## 2. Workspace Layout

Start with three crates and grow only when a real ownership or reuse need appears:

```text
Cargo.toml                     # Virtual workspace, shared dependencies and lints
Cargo.lock                     # Committed dependency resolution
rust-toolchain.toml            # Exact development compiler and components
crates/
  sf-cli/                      # sf-rust binary, argument parsing and rendering
    src/
    tests/                     # Executable-level integration tests
  sf-core/                     # Command workflows, configuration and authentication
    src/
    tests/
  sf-api/                      # Salesforce transport, wire models and pagination
    src/
    tests/
      fixtures/                # Synthetic protocol fixtures; never live credentials
```

The binary is named `sf-rust` during development so that tests cannot accidentally
replace or invoke the reference `sf`. A later `sf` alias must be an explicit user
installation choice. Put integration tests inside a member crate; a virtual
workspace root does not own an executable integration-test target.

Do not introduce general-purpose `sf-utils`, `sf-types`, or per-command crates in
advance. Keep models beside the behavior that owns them and extract a shared
module only when actual callers justify it. Dependency direction and interfaces
are specified in the [architecture](technical-architecture.md).

## 3. Dependency Policy

| Purpose | Initial choice and constraint |
|---|---|
| Argument parsing | `clap` with derive; explicitly adapt parsing errors and output to the supported compatibility contract. |
| Async work | `tokio`; enable only needed features. Use one runtime owned by the binary. |
| HTTP | `reqwest` with explicit TLS and feature selection; reuse a client rather than constructing one per request. |
| Serialization | `serde`, `serde_json`; `quick-xml` for typed, escaped XML handling. |
| Errors | `thiserror` for typed library errors; `anyhow` only at the executable orchestration layer where useful. |
| Diagnostics | `tracing` and `tracing-subscriber`; stderr output, redaction, and no secret-bearing debug dumps. |
| Credentials | `keyring` for interactive OS storage; `secrecy`/`zeroize` for appropriately owned in-memory secrets. |
| Tests | `wiremock`, `assert_cmd`, `tempfile`, and `proptest` where their behavior is needed. |

These are choices of responsibility, not fixed dependency versions. The first
manifest change must select compatible maintained releases, inspect default
features, and record the lockfile. In particular, selecting a Rustls feature must
not accidentally retain an unwanted default TLS backend. Centralize features at
the workspace where practical; do not enable broad feature sets without a use.

Every new dependency needs a purpose, license check, maintenance review, and MSRV
check. Prefer standard-library facilities when they satisfy the requirement
without duplicating substantial protocol or security logic.

## 4. Rust Coding Contract

### Interfaces, types, and ownership

- Keep the public interface small: private fields/modules by default, `pub(crate)`
  for internal collaboration, and documentation for exported library items.
- Represent invalid states with enums, validated constructors, and newtypes where
  they prevent real mistakes, such as mixing org identifiers, aliases, and URLs.
  Use wire-specific structs at the transport seam rather than passing untyped
  JSON through every module.
- Prefer borrowing and slices when callers need no ownership transfer. Clone only
  when ownership or concurrency requires it; measure before introducing caches,
  reference counting, or other performance complexity.
- Use standard traits and idiomatic iterators. Avoid a trait for every struct,
  speculative generic frameworks, or a method that merely forwards to another
  method without hiding useful behavior.
- Validate user and server input at the owning interface. Conversions must not
  silently truncate, discard errors, or replace malformed data with defaults.

### Errors and unsafe code

- Production code MUST propagate recoverable failures with `Result` and `?`.
  Library errors preserve actionable categories and safe context. The CLI maps
  them to tested exit codes and, when requested, the compatibility JSON envelope.
- Do not use `unwrap`, `expect`, deliberate panics, or indexing assumptions on
  untrusted input. Do not ignore `Result`, swallow errors, or print an error and
  then report success. Error chains must not expose credentials or response bodies
  containing sensitive data.
- Forbid unsafe code in project crates. Third-party unsafe code is not prohibited
  by that lint; dependencies still need review. Any future first-party exception
  requires an explicit design change, safety rationale, and focused verification.
- Tests may use `expect` with descriptive messages through narrowly scoped,
  explained lint allowances. Broad production lint suppression is not acceptable.

### Async behavior and resources

- Keep blocking filesystem, CPU-heavy archive/XML work, and blocking credential
  operations off async executor threads using bounded blocking work.
- Do not hold a synchronous lock guard across `.await`. Bound request concurrency,
  queues, buffered bodies, subprocess output, and retry budgets.
- Give operations deadlines and cancellation behavior. Track and join spawned
  tasks; do not leave detached work mutating state after a command exits.
- Cancellation is not rollback: a remote deploy or other write may already have
  been accepted. Preserve its job identifier and make recovery/reporting explicit.
- Use structured tracing with useful operation identifiers, but never tokens,
  authorization headers, private keys, or whole configuration/authentication data.

## 5. Required Lint Configuration

The root manifest MUST define these workspace lints:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
dbg_macro = "deny"
await_holding_lock = "deny"
```

Every member MUST opt in; defining workspace lints alone does not apply them:

```toml
[lints]
workspace = true
```

Library crates SHOULD enable `#![warn(missing_docs)]`; their public interfaces
must document errors and important invariants. CI treats warnings as errors.
Do not enable all of Clippy's `pedantic` or `restriction` groups indiscriminately.
Add individual lints when they enforce a useful, agreed rule.

## 6. Setup and Required Developer Checks

**Today:** there is no Cargo workspace to build. Do not describe the following as
working setup steps until the bootstrap deliverables exist.

**After bootstrap:**

1. Install Rust through the [official rustup instructions](https://rustup.rs/),
   inspect the workspace's pinned toolchain, and install the documented platform
   prerequisites for TLS and credential storage.
2. Install the exact versions of `cargo-audit`, `cargo-deny`, and `cargo-llvm-cov`
   recorded by the bootstrap CI/tooling configuration. Tool installation must
   specify `--version` and `--locked`, not silently fetch the newest executable.
3. Run the required commands below from the workspace root.
4. Run `cargo run -p sf-cli --bin sf-rust -- --help`. Ordinary development and tests
   require no Salesforce credentials. Use only a dedicated test org for opt-in
   live tests; never put tokens or passwords in configuration JSON.

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo audit
cargo deny check
```

The environment assignment above uses POSIX shell syntax; Windows jobs must set
the equivalent environment variable. `cargo test --workspace` includes doctests
as well as member-crate tests. Feature variants and the MSRV job are additional
checks described in the [testing strategy](testing-strategy.md), not substitutes
for this default-feature baseline.

The bootstrap change MUST supply the manifest, toolchain pin, lockfile, lint
inheritance, audit/license policy, and CI jobs before implementation is accepted.
Repository rules must require those checks to pass; a prose command list or an
optional workflow does not enforce a standard.

## 7. Primary Rust References

The requirements above are this project's policy. These first-party references
define the underlying tooling behavior:

- [Cargo workspaces and lint inheritance](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table).
- [Cargo MSRV declaration](https://doc.rust-lang.org/cargo/reference/rust-version.html),
  [Edition 2024](https://doc.rust-lang.org/edition-guide/rust-2024/index.html), and
  [resolver versions](https://doc.rust-lang.org/cargo/reference/resolver.html#resolver-versions).
- [rustup toolchain files](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file):
  pinning the development compiler is distinct from declaring and testing an MSRV.
- [Cargo lockfiles](https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html)
  and [`cargo build --locked`](https://doc.rust-lang.org/cargo/commands/cargo-build.html#manifest-options).
- [rustfmt check mode](https://github.com/rust-lang/rustfmt#readme)
  and [Clippy usage and lint groups](https://github.com/rust-lang/rust-clippy#readme).
- [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown):
  cancellation and task completion require deliberate coordination.
