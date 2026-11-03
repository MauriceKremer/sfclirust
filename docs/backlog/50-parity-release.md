# 5. First-Party Parity and Release

**Goal:** expand supported commands deliberately, quantify compatibility, and
ship verified native artifacts.

**Depends on:** foundational and workflow work. Each command is gated by the
modules it actually uses; this package must not block an independently complete
command from being released.

## Relationship to the B40 developer preview

B20-05, B30-05, and B40-03/04 must provide the minimum executable and pinned
compatibility evidence for their own commands. B40-05 owns the integrated review,
native/live walkthrough, and green-CI gate. These requirements are pulled
forward, not deferred to completion of this package.

B50-02 still owns broader automated oracle installation/comparison and baseline
upgrade coverage; reuse any preview harness/fixtures rather than duplicating
them. B50-03 owns public packaging/publication and release integrity. A runnable
build-from-source B40 preview is not a claim that those release tasks are done.

## B50-01: Grow command coverage from the inventory

- [ ] Rank first-party workflows by user value, dependency reuse, and risk.
- [ ] Port each selected command end-to-end, including argument validation,
  context resolution, operation, JSON/human output, errors, and recovery.
- [ ] Record explicit status and native-only exclusions for every baseline
  bundled, JIT, and framework command.
- [ ] Treat npm plugin management, JIT installation, arbitrary JavaScript hooks,
  and legacy plugin loading as unsupported unless a separately approved native
  replacement is designed.

**Likely files**

- `docs/compatibility/commands.yaml`
- `crates/sf-cli/src/commands/<topic>/`
- Corresponding `crates/sf-core/src/<workflow>/`
- Corresponding `crates/sf-cli/tests/` and `crates/sf-core/tests/`

**Acceptance**

- A command becomes `supported` only when offline, differential, negative, and
  required live contracts pass.
- Public support statements match the inventory; command count alone is not a
  parity metric.

## B50-02: Automate differential compatibility testing

- [ ] Install the pinned upstream distribution separately from `sf-rust`.
- [ ] Isolate home/state/project copies and never share mutable auth state.
- [ ] Compare structured JSON, exit status, stdout/stderr, and defined side
  effects. Normalize only reviewed volatile fields.
- [ ] Store only synthetic or sanitized fixtures and record why each fixture
  changed.

**Likely files**

- `tests/compatibility/`
- `tests/fixtures/compatibility/`
- `.github/workflows/compatibility.yml`
- `docs/compatibility/README.md`

**Acceptance**

- A baseline update produces a reviewable diff; normal application builds do not
  install or run Node.js.
- A changed contract fails CI until the behavior or an intentional documented
  difference is reviewed.

## B50-03: Harden release and platform packaging

- [ ] Build/test on native Linux, both supported macOS architectures, and
  Windows MSVC.
- [ ] Confirm OS-specific secure store, trust roots, proxy/CA, paths, permissions,
  signals, and installer behavior on packaged binaries.
- [ ] Publish checksums, provenance, dependency/license inventory, release
  notes, platform requirements, and rollback instructions.
- [ ] Keep self-update and `sf` alias installation out of scope until explicitly
  designed and reviewed.

**Likely files**

- `.github/workflows/release.yml`
- `packaging/` (only for supported formats)
- `docs/release.md`
- `CHANGELOG.md`
- `THIRD_PARTY_NOTICES.md`

**Acceptance**

- Native smoke tests run the release artifacts, not just development executables.
- Advertised commands, platforms, limitations, and upstream baseline are
  published accurately.
- Security, live-test, dependency, and compatibility release gates pass.

## B50-04: Measure performance and resource limits

- [ ] Establish fixed CI/dev hardware and representative synthetic workloads.
- [ ] Measure startup, peak memory, artifact size, large-result streaming, and
  representative metadata work separately from server/network latency.
- [ ] Set budgets from measured baseline; add benchmarks only for behavior that
  matters to users.
- [ ] Add resource-limit regression tests for body size, concurrency, retries,
  archives, and polling.

**Likely files**

- `crates/sf-cli/benches/`
- `crates/sf-api/benches/`
- `docs/performance.md`
- CI benchmark workflow only if results are stable and actionable.

**Acceptance**

- Performance claims identify equivalent work, hardware, sample size, and
  variance.
- Resource bounds fail safely and do not silently truncate valid results.
