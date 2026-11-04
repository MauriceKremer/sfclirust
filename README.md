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

`sf-rust` is transitioning from the **foundation milestone (M0)** to the
**authenticated read workflows milestone (M1)**. The workspace, toolchain,
runtime foundations, authenticated API transport primitives, REST/pagination
models, and safe retry/rate-limit policies are implemented. Project loading and
the initial ApexClass/ApexTrigger mapping/archive layer also exist. These are
library capabilities: browser login, persistent OS credentials, and the first
authenticated CLI workflows are not yet available. `org list` currently returns
an explicit unsupported error.

| Area | State |
|---|---|
| Cargo workspace, pinned toolchain, MSRV, `Cargo.lock` | In place |
| Enforced lints (`unsafe` forbidden, no `unwrap`/`expect`/`dbg`) | In place |
| CI: fmt, clippy, test, doc, audit, deny, coverage, native-host check | Defined; B00-06 repairs and verifies failures/enforcement |
| Pinned compatibility baseline + command inventory | In place |
| Minimal `sf-rust` binary (`--help`, `--version`, offline help) | In place, verified |
| First-party commands (M1+ slice) | **In progress** |
| Required CI as *blocking* merge rules | `main` requires the strict `Required CI` aggregate; passing remote run pending |

Verified in this working tree:

```console
$ cargo run -p sf-cli --bin sf-rust -- --version
sf-rust 0.1.0

$ cargo test --workspace --locked
# Ordinary tests use isolated state; no Salesforce credentials required
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

### Authentication commands (developer preview, not yet `supported`)

Browser login, `org list`, `org display`, and `org logout` run natively but are
still `planned` in the compatibility inventory until pinned comparison and live
evidence exist (see `docs/backlog/20-state-auth.md`).

- Browser login needs your own OAuth app in the target org (see
  [Creating the OAuth app](#creating-the-oauth-app-for-browser-login)). Pass its
  Consumer Key with `--client-id` or `SF_RUST_CLIENT_ID`. Tokens go to the OS secure
  store; if it is unavailable the command fails (there is no plaintext fallback).
- The callback listens on port 1717 by default (the upstream CLI's port). Override it
  with `--callback-port` or `SF_RUST_CALLBACK_PORT` and register the matching URL.
- Headless: set `SF_ACCESS_TOKEN='<org id>!<token>'` and `SF_RUST_INSTANCE_URL`.
  Nothing is persisted, and the target org must be the token's org ID. Currently
  only `org display` reads it.
- Behind a TLS-inspecting proxy (for example Netskope), requests fail with an
  untrusted-issuer error until you trust the proxy's CA. Point `SF_RUST_CA_BUNDLE` (or
  `NODE_EXTRA_CA_CERTS`, which the upstream CLI also honors) at a PEM file with that CA.
  Certificate verification is never disabled. On macOS, export the CA with
  `security find-certificate -a -c "<CA name>" -p /Library/Keychains/System.keychain`.
- `org logout` needs `--no-prompt` and removes local credentials only; it does not
  revoke tokens remotely.
- `SF_RUST_STATE_DIR` relocates native state. `SF_RUST_CREDENTIAL_BACKEND=memory`
  is for tests only. Ctrl-C or SIGTERM stops a command with exit 130 or 143.

#### Creating the OAuth app for browser login

Browser login uses the OAuth authorization-code flow with PKCE and a loopback
callback. It is a public client: it sends no client secret. Create the app once per
org (use a disposable org for development). These settings were used for a working
login with an External Client App; the same values apply to a classic Connected
App where your org still allows creating one.

1. In Setup, open **External Client Apps Manager** and create a new external client
   app (name and contact email are free-form).
2. Enable OAuth and set:
   - **Callback URL:** `http://localhost:1717/oauth/callback` (exactly; no
     trailing slash). Salesforce redirects to the registered URL, not to the URL
     the CLI sends, so the port must be registered. If you use another port, pass
     `--callback-port <port>` and register that port instead.
   - **OAuth scopes:** `api` (Manage user data via APIs) and `refresh_token,
     offline_access` (Perform requests at any time). Nothing else is requested.
   - **Flow enablement:** turn on **Enable Authorization Code and Credentials
     Flow**. Leave Client Credentials, Device, JWT Bearer and Token Exchange off.
   - **Security:** require Proof Key for Code Exchange (PKCE). Turn off **Require
     secret for web server flow** and **Require secret for refresh token flow**.
3. Save, then on the **Policies** tab allow the users who will log in (for example
   "All users may self-authorize" in a disposable org, or an assigned profile or
   permission set).
4. Copy the **Consumer Key**. It is not a secret, but keep it in an environment
   variable rather than a committed file. The Consumer Secret is not used.
5. Run the browser login with that key. For a sandbox or My Domain org, also pass
   `--instance-url` with the org's own host:

   ```bash
   export SF_RUST_CLIENT_ID='<consumer key>'
   sf-rust org login web --alias dev --set-default
   ```

Troubleshooting:

| Symptom | Likely cause |
|---|---|
| `invalid_client_id` | Wrong login host for the org that owns the app (pass `--instance-url`), key copied incorrectly, or the new app has not propagated yet (wait a few minutes). |
| `redirect_uri_mismatch` | The registered callback does not match `http://localhost:<port>/oauth/callback`, including the port. |
| OAuth error 1800 | The app is not fully set up: authorization-code flow not enabled, or a required setting missing. |
| Browser shows "localhost refused to connect" | The login command is not running or already ended (timeout, Ctrl-C), or another process holds the port. Start a fresh run and use only the tab it opens. |
| Port already in use | Another login is running on the same port; wait for it or choose `--callback-port`. |

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

### Next delivery checkpoint: runnable B40 preview

The [revised B40 plan](docs/backlog/README.md#b40-delivery-target-runnable-developer-preview)
first repairs CI, then closes secure credential storage and real auth/org/query
command integration before completing deploy/retrieve. After implementation,
B40-05 requires a combined review, a build-from-source login-to-logout walkthrough
(including query and metadata operations), and green required CI on the delivered
revision. Until that gate passes, this remains an implementation target, not an
available command set or a public packaged release.

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
