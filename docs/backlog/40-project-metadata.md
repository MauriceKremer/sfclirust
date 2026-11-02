# 4. Salesforce Projects and Metadata Workflows

**Goal:** understand real Salesforce project layouts and deliver safe,
recoverable source/metadata operations.

**Depends on:** CLI contract, native state/auth, and the transport workflows the
selected commands actually need.

## B40-01: Load Salesforce project context

- [ ] Define supported `sfdx-project.json` fields against the pinned upstream
  schema/source. Preserve unknown fields; avoid speculative schema copies.
- [ ] Resolve project root, package directories, default API version, manifest,
  and source paths with explicit path validation.
- [ ] Report missing/malformed project context using the agreed CLI error
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

- [ ] Determine the exact initial metadata types from the first deploy/retrieve
  command contracts. Do not claim general metadata parity before coverage exists.
- [ ] Implement source/metadata conversion using typed models and reviewed
  fixtures; document known unsupported types.
- [ ] Validate archive paths, symlinks, extraction root, compressed/expanded
  size, entry count, and decompression work before writing files.

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

- [ ] Implement a first supported validate/deploy operation with flag validation,
  org selection, async/sync mode, timeout, and dry-run behavior as inventoried.
- [ ] Preserve deployment/job identifiers and represent queued, in-progress,
  succeeded, failed, and partial outcomes distinctly.
- [ ] Implement bounded polling and interruption reporting. Never claim that
  cancelling the local wait rolled back the remote deployment.

**Likely files**

- `crates/sf-core/src/deploy/mod.rs`
- `crates/sf-core/src/deploy/submit.rs`
- `crates/sf-core/src/deploy/status.rs`
- `crates/sf-cli/src/commands/project/deploy/`
- `crates/sf-core/tests/deploy_lifecycle.rs`

**Acceptance**

- Wire tests cover validation failure, partial error, HTTP/SOAP errors, polling
  timeout, interruption, resume/reporting, and uncertain submission.
- One disposable-org test proves the end-to-end operation before release.

## B40-04: Implement retrieval and filesystem materialization

- [ ] Add retrieval submission/polling tied to a supported metadata selection.
- [ ] Safely map response files into the requested project without overwriting
  unrelated files silently.
- [ ] Define atomicity/recovery behavior for partial local writes and cleanup.

**Likely files**

- `crates/sf-core/src/retrieve/`
- `crates/sf-cli/src/commands/project/retrieve/`
- `crates/sf-core/tests/retrieve_lifecycle.rs`

**Acceptance**

- Tests cover empty result, unsupported type, collision, partial download, disk
  failure, path traversal, and final file layout.
- Live test uses disposable resources and cleans up on success/failure.
