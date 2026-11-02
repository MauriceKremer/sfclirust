# sf-rust

A native **Rust** reimplementation of the [Salesforce CLI](https://github.com/salesforcecli/cli)
(`sf` / `sfdx`), delivered in vertical slices toward first-party command parity
against a pinned upstream baseline.

This is a *behavioral* port, not a TypeScript-to-Rust translation: it ports the
observable behavior, contracts, and safety properties of the CLI, while
improving startup cost, distribution simplicity, and predictable resource use.
The shipped binary is **native-only** — it has no Node.js runtime, no automatic
fallback to `sf`, and no execution of legacy JavaScript `oclif` plugins.

> This is an independent project. It is not affiliated with, endorsed by, or part
> of Salesforce, Inc.

---

## Status today

`sf-rust` is at the **foundation milestone (M0)**: a reproducible, pinned,
linted, and tested Rust workspace with a minimal executable. No first-party
command is implemented yet.

| Area | State |
|---|---|
| Cargo workspace, pinned toolchain, MSRV, `Cargo.lock` | In place |
| Enforced lints (`unsafe` forbidden, no `unwrap`/`expect`/`dbg`) | In place |
| CI: fmt, clippy, test, doc, audit, deny, native-host check | In place |
| Pinned compatibility baseline + command inventory | In place |
| Minimal `sf-rust` binary (`--help`, `--version`, offline help) | In place, verified |
| First-party commands (M1+ slice) | **Not started** |
| Required CI as *blocking* merge rules | Open (last M0 item) |

Verified in this working tree:

```console
$ cargo run -p sf-cli --bin sf-rust -- --version
sf-rust 0.1.0

$ cargo test --workspace --locked
# 4 smoke tests pass; no credentials or network required
```

The normative design documents describe *requirements to satisfy*, not gates that
already pass end-to-end. A command is considered **supported** only after its
compatibility inventory entry and required contract tests pass.

---

## Quick start

You need [rustup](https://rustup.rs/). The development toolchain is pinned in
`rust-toolchain.toml` (stable **1.99.1**, with `rustfmt` and `clippy`), so
`rustup` selects it automatically when you enter the directory. Building and
running `--help`/`--version` needs no Salesforce credentials.

```sh
# Verify the binary runs offline
cargo run -p sf-cli --bin sf-rust -- --help
cargo run -p sf-cli --bin sf-rust -- --version
```

Run the full check suite from the workspace root (all builds use `--locked` for
reproducible resolution):

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo audit
cargo deny check
```

Ordinary development and tests require **no** Salesforce credentials and no
network access. Live-org suites are opt-in and use disposable, least-privilege
orgs.

---

## Architecture

Three crates, one dependency direction:

```text
sf-cli  -->  sf-core  -->  sf-api
   |            |             |
   |            |             +-- HTTP / REST / Metadata SOAP,
   |            |                 wire models, pagination
   |            +-- workflows, auth, config precedence,
   |                 project interpretation
   +-- arguments, terminal I/O, JSON/human rendering, exit mapping
```

- **`sf-cli`** — the `sf-rust` binary, command dispatch, a single async runtime,
  signals, stdin/stdout/stderr, and output/exit rendering. Parsed input becomes
  typed requests; typed outcomes become output and exit codes. Workflow code
  never prints directly or terminates the process.
- **`sf-core`** — authentication, org selection, configuration precedence,
  project loading, and command workflows. Its interfaces hide the sequencing an
  operation needs rather than exposing dozens of low-level calls.
- **`sf-api`** — reusable HTTP clients, REST/Tooling and Metadata SOAP
  transport, wire types, pagination, and transport-level failure classification.
  It accepts an authenticated session and transport settings; it does not parse
  CLI input, print output, choose the default org, or read the user's home.

Interfaces are deliberately small and deep: callers exercise meaningful behavior
without reproducing implementation details, and tests cross those same
interfaces. General-purpose utility or type crates are added only when a real
ownership or reuse need appears.

The development binary is named **`sf-rust`** so tests can never accidentally
replace or invoke the installed reference `sf`. A later `sf` alias must be an
explicit user installation choice.

---

## Compatibility approach

Parity is defined against a **pinned upstream baseline**, not a moving `latest`:

- **Baseline:** `@salesforce/cli@2.158.4`, release commit
  `8ca3ae36d1` ([`upstream-baseline.yaml`](docs/compatibility/upstream-baseline.yaml)).
- **Inventory:** the first vertical slice and its per-command contracts live in
  [`commands.yaml`](docs/compatibility/commands.yaml).
- **No success-shaped stubs.** Every unregistered command fails with a clear
  nonzero "unsupported" error; the binary never silently shells out to another
  executable.
- **Differential testing** compares the pinned upstream CLI and `sf-rust` against
  equivalent isolated inputs, comparing the declared contract (JSON shape, field
  types, null-vs-absent, exit status, side effects) rather than assuming
  byte-identical output. Node.js exists *only* as the test oracle, never as a
  runtime dependency.
- **Intentional differences are public.** Where native behavior diverges from
  upstream (for example, a separate native credential store), it is documented
  with a reason and a test — never claimed as parity.

The M1 vertical slice, a complete authenticate → verify → operate → clean-up
workflow:

| # | Command | Purpose |
|---|---|---|
| 1 | `sf org login web` | Browser-based OAuth authentication |
| 2 | `sf org logout` | Credential revocation and cleanup |
| 3 | `sf org list` | List authenticated orgs |
| 4 | `sf org display` | Display org metadata |
| 5 | `sf data query` | Execute a SOQL query |

---

## Roadmap

| Phase | Scope | Exit criteria (summary) |
|---|---|---|
| **M0 · foundation** | Workspace, toolchain/MSRV, lints, CI, binary identity, baseline inventory, output/error contracts | Dev-guide checks implemented and required; offline smoke test passes on supported hosts; no credentials for ordinary tests |
| **M1 · authenticated read workflows** | Browser + ephemeral token login, logout, alias/config, org list/display, SOQL query | Positive/negative compatibility tests per command; JSON/exit and credential lifecycle verified; a disposable-org test proves the end-to-end slice |
| **M2 · project & metadata workflows** | Project/package/manifest interpretation, source conversion, deploy/validate/report/retrieve, async lifecycle | Synthetic fixtures and live test orgs cover supported types, partial failure, recovery, cancellation, and safe writes |
| **M3 · expand first-party parity** | Org provisioning, Apex/testing, data/Bulk, packaging, remaining inventoried commands | Each command graduates separately through the compatibility and security gates |

Progress is measured against the pinned inventory, not the number of registered
command names. The ordered work that produces these milestones lives in
[`docs/backlog`](docs/backlog/README.md).

---

## Security and supply-chain posture

The design bakes in controls that matter for a tool handling org credentials:

- **Credentials** persist only in the OS secure store (`keyring`: macOS Keychain,
  Windows Credential Manager, a Linux secret store) or an explicit ephemeral
  headless mode. A locked, missing, or unsupported store yields an actionable
  error — never a plaintext fallback.
- **State is native and isolated.** `sf-rust` uses its own per-user data
  namespace with an `SF_RUST_STATE_DIR` override; it does not read, decrypt, or
  rewrite the reference CLI's `~/.sf` / `~/.sfdx` auth files.
- **Secrets never leak** into logs, traces, error chains, snapshots, or build
  artifacts; secret-bearing types use `secrecy`/`zeroize`.
- **Network is constrained to HTTPS/TLS ≥ 1.2** with verification; cross-origin
  redirects and response URLs never receive credentials automatically.
- **Unsafe is forbidden** in first-party crates; `unwrap`/`expect`/`dbg` and
  holding a lock across `.await` are denied by workspace lints.
- **Supply chain** is governed by a committed `Cargo.lock`, `cargo audit`, and a
  `cargo deny` policy (`deny.toml`): all dependencies must declare a license,
  come from crates.io only, and carry no known advisories.

Full requirements are specified in
[`docs/security-compliance.md`](docs/security-compliance.md).

---

## Repository layout

```text
Cargo.toml                     # virtual workspace, shared deps, lints
Cargo.lock                     # committed dependency resolution
rust-toolchain.toml            # pinned development toolchain (1.99.1 + fmt/clippy)
deny.toml                      # license / source / advisory policy
crates/
  sf-cli/                      # sf-rust binary, parsing, rendering, exit mapping
    src/{main.rs,lib.rs}
    tests/                     # executable-level integration (assert_cmd)
  sf-core/                     # workflows, config, authentication
  sf-api/                      # transport, wire models, pagination
docs/
  technical-architecture.md    # modules, dependency direction, compatibility contract
  development-setup.md         # toolchain, lints, required checks
  testing-strategy.md          # test layers, differential rules, release gates
  security-compliance.md       # credentials, trust, files, network, licensing
  backlog/                     # ordered implementation work packages
  compatibility/              # pinned baseline + first-party command inventory
.github/workflows/ci.yml      # fmt, clippy, test, doc, audit, deny, host checks
AGENTS.md                      # routing for AI agents working in this repo
```

---

## Documentation

The `docs/` set is normative for future changes; `AGENTS.md` routes agents to it.

| Trigger | Source of truth |
|---|---|
| Choosing or scoping work | [`docs/backlog/README.md`](docs/backlog/README.md) |
| Crate layout, toolchain, lints, checks | [`docs/development-setup.md`](docs/development-setup.md) |
| Modules, dependency direction, compatibility contract | [`docs/technical-architecture.md`](docs/technical-architecture.md) |
| Tests, differential comparison, release gates | [`docs/testing-strategy.md`](docs/testing-strategy.md) |
| Credentials, trust, files, network, licensing | [`docs/security-compliance.md`](docs/security-compliance.md) |

### Working in this repo

This repository is agent-friendly. Before touching a command, read
[`AGENTS.md`](AGENTS.md) and the governing docs, then pick the lowest-numbered
open task in the backlog whose dependencies are complete. Work is test-first
through the module's public interface, verified with the checks above, and
closed in the same change. The binary is always run as `sf-rust`; the reference
CLI is reachable only through the pinned oracle path in compatibility tests.

---

## License

Apache-2.0, matching the upstream CLI's license. Upstream-derived material keeps
its required attribution. Licensing and attribution obligations for ported code
and third-party dependencies are tracked in
[`docs/security-compliance.md`](docs/security-compliance.md) and will be recorded
in a `THIRD_PARTY_NOTICES` file before release.
