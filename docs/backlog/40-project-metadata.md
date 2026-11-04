# 4. Salesforce Projects and Metadata Workflows

**Goal:** deliver a runnable native developer preview with authentication,
authenticated reads, and safe, recoverable source/metadata operations. Finish
with an integrated review and green required CI, not only library completion.

**Depends on:** CLI contract, native state/auth, and the transport workflows the
selected commands actually need. Follow the
[B40 delivery sequence](README.md#b40-delivery-target-runnable-developer-preview):
B00-06 repairs CI, B20-04/05 deliver real auth, and B30-05 delivers the initial
query command. B40-01/02 are implemented foundations, not CLI support claims.

## B40-01: Load Salesforce project context

- [x] Define supported `sfdx-project.json` fields against the pinned upstream
  schema/source. Preserve unknown fields; avoid speculative schema copies.
- [x] Resolve project root, package directories, default API version, manifest,
  and source paths with explicit path validation.
- [x] Report missing/malformed project context using the agreed CLI error
  contract.

**Likely files**

- `crates/sf-core/src/project/mod.rs`
- `crates/sf-core/src/project/project_json.rs`
- `crates/sf-core/src/project/paths.rs`
- `crates/sf-core/tests/project_loading.rs`
- `crates/sf-core/tests/fixtures/projects/`

**Acceptance**

- Tests cover valid, missing, malformed, multi-package, and unsupported projects
  on Windows/macOS/Linux path conventions.
- Loading does not rewrite user project files.

## B40-02: Build metadata mapping and archive safety

- [x] Determine the exact initial metadata types from the first deploy/retrieve
  command contracts. Do not claim general metadata parity before coverage exists.
- [x] Implement source/metadata conversion using typed models and reviewed
  fixtures; document known unsupported types.
- [x] Validate archive paths, symlinks, extraction root, compressed/expanded
  size, entry count, and decompression work before writing files.

The first conversion slice covers only `ApexClass` and `ApexTrigger`, including
their required `-meta.xml` sidecars. Deploy inputs are limited to source or
manifest selections that resolve entirely to these types; retrieve selections
are limited to these types and concrete member names. Full CLI selector, flag,
and output contracts remain planned for B40-03/B40-04 and must be inventoried
before those commands are implemented. Other metadata types, wildcard members,
and non-`main/default` source layouts are explicitly unsupported by this
converter.

**Likely files**

- `crates/sf-api/src/metadata/` (wire types/conversion where owned by protocol)
- `crates/sf-core/src/project/metadata/` (project mapping/workflow)
- `crates/sf-core/tests/metadata_mapping.rs`
- `crates/sf-core/tests/fixtures/metadata/`

**Acceptance**

- Synthetic fixtures cover supported source/metadata forms and error paths.
- Archive traversal, symlink, and resource exhaustion cases fail safely.
- No retrieved file is executed as a hook or script.

## B40-03: Implement deployment submission and status lifecycle

**Depends on:** B20-05, B30-01/03/04, and B40-01/02.

**Progress (partial, task remains open):** `crates/sf-api/src/metadata/deploy.rs`
holds the wire layer: `deploy` and `checkDeployStatus` request builders,
`DeployId` validation, `DeployStatus` (terminal vs non-terminal, unknown values
rejected), `DeployResult` with component failures, SOAP fault surfacing. Covered by
`crates/sf-api/tests/metadata_deploy.rs`. Not started: command inventory, archive
assembly, HTTP submission to the Metadata SOAP endpoint, polling, resume state,
CLI command, live test. B20-05 is itself still open.

- [ ] Inventory exact `project deploy start` and necessary report/resume command
  contracts against the pinned baseline before implementation. Target validation
  through the start command's dry-run mode; standalone validate/quick-deploy
  parity is outside this preview unless separately inventoried and completed.
  Limit source/manifest selection to B40-02's supported types and concrete members.
- [ ] Implement a first supported validate/deploy operation with flag validation,
  org selection, async/sync mode, timeout, and dry-run behavior as inventoried.
- [ ] Assemble a bounded deployment archive including `package.xml` and required
  sidecars; wire the authenticated Metadata API operations and response models
  needed by the selected commands. Reuse the B20-05 production runtime/auth path.
- [ ] Preserve deployment/job identifiers and represent queued, in-progress,
  succeeded, failed, and partial outcomes distinctly.
- [ ] Implement bounded polling and interruption reporting. Never claim that
  cancelling the local wait rolled back the remote deployment.
- [ ] Make reporting/resume work from a new process using an explicit job ID
  and validated org context; persist any recovery state only in native storage.
  Never resubmit a mutation whose acceptance is uncertain.

**Likely files**

- `crates/sf-core/src/deploy/mod.rs`
- `crates/sf-core/src/deploy/submit.rs`
- `crates/sf-core/src/deploy/status.rs`
- `crates/sf-api/src/metadata/` (required deployment wire operations)
- `crates/sf-cli/src/commands/project/deploy/`
- `crates/sf-core/tests/deploy_lifecycle.rs`
- `crates/sf-cli/tests/` (deployment executable/compatibility contracts)

**Acceptance**

- Wire tests cover validation failure, partial error, HTTP/SOAP errors, polling
  timeout, interruption, resume/reporting, and uncertain submission.
- Executable tests cover selected flags, archive contents/limits, org/project
  selection, async return, new-process report/resume, JSON/exit behavior, and
  real interruption with recovery information. Required pinned compatibility
  comparisons pass before any command is marked `supported`.
- One opt-in disposable-org test proves validation and deployment before B40
  closure. Preserve an available job ID on failure; missing evidence leaves
  B40-05 open.

## B40-04: Implement retrieval and filesystem materialization

**Depends on:** B40-03's integrated metadata/runtime path, B40-01/02, B20-05,
and B30-04.

- [ ] Inventory `project retrieve start` and any status/resume operation required
  by its selected async/wait contract. Explicitly reject unsupported selectors,
  metadata types, and flags rather than partially retrieving them.
- [ ] Add retrieval submission/polling tied to a supported metadata selection.
- [ ] Complete bounded archive download/decoding and authenticated wire models;
  reuse B40-02 validation before any project writes.
- [ ] Safely map response files into the requested project without overwriting
  unrelated files silently.
- [ ] Define atomicity/recovery behavior for partial local writes and cleanup.
  State the collision/overwrite policy, stage and validate before replacement,
  and preserve enough information to recover if a multi-file update fails.

**Likely files**

- `crates/sf-core/src/retrieve/`
- `crates/sf-api/src/metadata/` (required retrieval wire operations)
- `crates/sf-cli/src/commands/project/retrieve/`
- `crates/sf-core/tests/retrieve_lifecycle.rs`
- `crates/sf-cli/tests/` (retrieval executable/compatibility contracts)

**Acceptance**

- Tests cover empty result, unsupported type, collision, partial download, disk
  failure, path traversal, and final file layout.
- Executable and pinned compatibility tests cover the complete selection,
  polling, interruption/recovery, output, and filesystem contract.
- An opt-in live test retrieves the disposable ApexClass/ApexTrigger slice and
  verifies its final project layout; cleanup runs on success/failure. Required
  before B40 closure, not postponed to a later release.

## B40-05: Review and accept the runnable B40 milestone

**Depends on:** B00-06, B20-04/05, B30-05, and B40-01 through B40-04.
Run after deploy/retrieve implementation, before B50 expansion or declaring
B40 complete. This is the requested post-B40 review and stabilization gate.

- [ ] Review the combined application changes since the last reviewed baseline,
  including earlier B10/B20/B30 work consumed by B40. Check the actual executable,
  not only the latest module diff, against the normative docs and command
  inventory. Fix milestone-blocking findings and add regression tests.
- [ ] Recheck credential persistence/redaction, wrong-org prevention, refresh and
  logout races, real signals and task cleanup, JSON/exit contracts, archive
  limits, uncertain submission, and local-write recovery. Do not accept a
  helper-only test as proof that a production handler uses it.
- [ ] Build `sf-rust` in the shipping release profile with `--locked`. Add
  executable smoke/contract checks for that actual binary, including offline
  help/version and explicit unsupported behavior for deferred commands.
- [ ] Demonstrate the complete user journey across separate processes:
  login -> list/display -> query -> validate/deploy -> report/resume -> retrieve
  -> logout. Verify org identity, persisted login reuse, retrieved files, and
  credential cleanup. Exercise the headless path separately without persistence.
- [ ] Run synthetic/offline and pinned compatibility tests in ordinary CI.
  Run native secure-store tests in isolated provisioned environments on every
  advertised target, and opt-in live workflows only against disposable orgs.
  Selecting a live suite without its prerequisites must fail, not skip as green.
- [ ] Run all required CI jobs on the exact final committed revision: formatting,
  build/check, strict Clippy, tests/doctests, rustdoc, audit, deny, 80% library
  coverage, MSRV, supported configurations, and native targets. Retain reports
  and verify that required merge checks remain enforced.
- [ ] Update the inventory and README with actual supported commands/flags,
  platform prerequisites, OAuth client setup, headless input, build/run examples,
  known limitations, and recovery/logout instructions. Record evidence and
  reviewed findings in the implementation note.

**Likely files/settings**

- `crates/sf-cli/tests/` and the workflow tests in `sf-core`/`sf-api`
- `.github/workflows/ci.yml` and isolated compatibility/live/platform workflows
- `docs/compatibility/commands.yaml` and reviewed fixtures
- `README.md`, `docs/development-setup.md`
- `docs/backlog/B40-05-IMPLEMENTATION.md` (created when executing this task)
- Repository required-check settings

**Acceptance**

- A developer following the README can build and run the initial functionality,
  including a browser login that remains usable by the next invocation.
- Every promised command has positive, negative, executable, and required
  compatibility/live evidence; unsupported commands/types remain explicit.
- Record a matrix of acceptance criterion, test command/result, platform,
  reviewed findings, and commit SHA, including the successful remote CI run.
  Native/live evidence must correspond to the delivered revision/build too.
- No unresolved milestone-blocking review findings or failing/skipped required
  jobs remain. Fixing review findings requires rerunning affected tests and the
  final gates; a green older SHA or a suppressed lint is not acceptance.
- Missing disposable-org credentials, native runner access, OAuth client setup,
  or repository permissions is recorded as a blocker and leaves B40-05 open.
