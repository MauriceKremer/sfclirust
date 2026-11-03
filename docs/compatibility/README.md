# Salesforce CLI Compatibility Baseline and Command Inventory

This directory documents the upstream baseline and first-party command inventory
for the `sf-rust` project. These files establish an immutable reference for
compatibility testing and vertical-slice implementation planning.

## Files in This Directory

### upstream-baseline.yaml

**Purpose:** Record an actual published Salesforce CLI distribution and its
installed/default plugin inventory.

**Contents:**
- CLI version and release commit (immutable)
- Platform and runtime requirements (Node.js version)
- Framework and host package details (@oclif/core, etc.)
- Bundled plugins with exact versions and licenses
- Core libraries (@salesforce/core, @salesforce/sf-plugins-core, etc.)
- Salesforce plugins (all bundled, never JIT)
- Just-in-time (JIT) plugins (installed on demand)
- Key transitive dependencies
- Lifecycle hooks
- Validation notes

**Key Properties:**
- All package versions and commit hashes are immutable and verifiable
- Bundled plugins have pinned versions; transitive dependencies may vary
- JIT plugins are not installed until explicitly requested
- Distribution may be verified by installing the same version from npm

**Usage:**
- Another developer can install `@salesforce/cli@2.158.4` from npm and verify
  the bundled plugins match this inventory
- This baseline is the reference for "what upstream shipped together"
- Source inspection of intermediate commits is documented separately in
  [technical-architecture.md](../technical-architecture.md#verified-source-map)

---

### commands.yaml

**Purpose:** Record the first vertical slice of commands to be ported and their
compatibility requirements.

**Contents:**
- Command identification (name, plugin, version)
- Command purpose and owner
- Flag definitions with types, aliases, and constraints
- Exit codes and error conditions
- Credential and state side effects
- Contracts that must be preserved (JSON schema, behavior, etc.)
- Dependencies and prerequisites
- Salesforce API endpoints and versions used
- Test requirements and coverage
- Exclusions and known limitations
- Migration notes from upstream
- A status (`planned`, `in-progress`, `supported`, or `intentionally-unsupported`)
  that prevents library primitives or parser registration from being mistaken
  for a verified end-to-end command

**Organization:**
Commands are grouped by **milestone** (M1, M2, M3, etc.) as defined in
[technical-architecture.md](../technical-architecture.md#3-milestones).

**Milestone 1 (M1):** Authenticated read workflows
- Candidate slice: browser and ephemeral token login, logout, alias/config,
  org list/display, and SOQL query
- Each recorded command must be verified end-to-end with positive/negative tests
- JSON/exit behavior and credential lifecycle are verified
- A disposable-org test proves the end-to-end slice

The current inventory entries are candidates, not claims of support. In
particular, implementing auth/session library primitives does not support
`sf org login web` or `sf org logout`; their command status stays `planned` until
the browser, secure credential storage, command output, and lifecycle are wired
and tested together.

**M1 First Slice Commands:**
1. `sf org login web` - Browser-based OAuth authentication
2. `sf org logout` - Credential revocation and cleanup
3. `sf org list` - List authenticated orgs
4. `sf org display` - Display org metadata
5. `sf data query` - SOQL query execution

**Usage:**
- Before implementing any command, add it to `commands.yaml` with full specification
- Test requirements are explicitly listed; they are acceptance criteria, not optional
- Dependency chains are recorded so downstream commands can be planned
- JIT plugins are listed but not counted as silently supported in the initial release

---

## Acceptance Criteria for B00-01

Task B00-01 ("Pin the upstream comparison baseline") is complete when:

✅ **Verified distribution:**
- An actual published Salesforce CLI distribution has been selected:
  **`@salesforce/cli@2.158.4`** from npm registry
- The release commit (`8ca3ae36d11a01726aec741897547adec3aa5ad2`) is verified
  as a merge commit for this version
- Another developer can install this exact version and verify the bundled
  plugins match the inventory without relying on `latest` or an ambiguous `PATH`

✅ **Immutable revisions and package integrity:**
- All bundled plugin versions are pinned and recorded in `upstream-baseline.yaml`
- Release commit hashes for key libraries are documented:
  - `@salesforce/sf-plugins-core`: `bc6bc6f14add235cdc739a38eb244c404c2d587c`
  - `@salesforce/plugin-deploy-retrieve`: `2e867d60f01a148fac1cbaf469136fed3314eaf3`
  - `forcedotcom/sfdx-core` (@salesforce/core): `23d7f942fa79b4a8eec9b75b4b9d97a425eb508b`
- npm registry URL for package integrity verification is documented
- Node.js requirement (`>=22.0.0`) is recorded

✅ **Baseline clearly distinguishes observed source commits from packages:**
- `upstream-baseline.yaml` documents the exact installed versions, not development snapshots
- Separate "Verified source map" section in
  [technical-architecture.md](../technical-architecture.md#verified-source-map)
  documents source discovery snapshots
- Bundled plugins are listed separately from JIT plugins

✅ **Command inventory with first vertical slice:**
- At least one vertical-slice candidate is recorded in `commands.yaml`
- Milestone 1 (M1) slice includes 5 commands forming a complete workflow:
  1. `sf org login web` (authentication entry point)
  2. `sf org logout` (cleanup)
  3. `sf org list` (verification)
  4. `sf org display` (introspection)
  5. `sf data query` (operation)
- JIT workflows are explicitly listed and marked as JIT (not silently supported)
- Each command records: purpose, flags, exit codes, side effects, contracts,
  dependencies, Salesforce API versions, test requirements, and exclusions

✅ **Attribution requirements verified:**
- All bundled plugins and core libraries are Apache-2.0 licensed
- LICENSE files checked for upstream CLI and key dependencies:
  - `salesforcecli/cli`: Apache-2.0
  - `forcedotcom/sfdx-core` (@salesforce/core): Apache-2.0
  - `salesforcecli/sf-plugins-core`: Apache-2.0
  - `salesforcecli/plugin-deploy-retrieve`: Apache-2.0
  - `forcedotcom/source-deploy-retrieve`: Apache-2.0
  - `@oclif/core`: Apache-2.0
- No NOTICE files found for upstream or key dependencies, but licensing
  obligations are documented for future THIRD_PARTY_NOTICES.md
- Transitive dependencies include MIT-licensed packages (e.g., `ansis`, `@inquirer/select`)

✅ **Build instructions require no Node.js:**
- Documented that Rust workspace build/test runs natively with `cargo check`
- Node.js is used only for running upstream oracle inside compatibility tests

---

## How to Use These Files

### For Implementers

1. **Before starting implementation:** Read the command entry in `commands.yaml`
2. **Understand the contracts:** The "contracts" section is not a suggestion;
   it defines the exact behavior expected by downstream workflows
3. **Plan your tests:** Every requirement in the "testing" section is an
   acceptance criterion
4. **Verify Salesforce API versions:** Each command documents the exact
   endpoints and versions it uses
5. **Check dependencies:** The "dependencies" section lists what libraries
   you need to consult for behavior and state management

### For Reviewers

1. **Verify immutability:** Check that all version strings and commit hashes
   match the npm registry and GitHub
2. **Verify completeness:** Every command must have:
   - Purpose and owner clearly stated
   - All flags with types and descriptions
   - Exit codes and error conditions
   - Side effects (including state/credential changes)
   - Contracts that must be preserved
   - Test requirements
3. **Verify no omissions:** JIT plugins are listed separately and not counted
   as bundled support
4. **Verify no copies:** Rust code is not derived from TypeScript upstream;
   only behavior is ported

### For Integration Testing

1. **Oracle invocation:** Use the pinned distribution documented in
   `upstream-baseline.yaml`
2. **Command reference:** Cross-check implementation output against upstream
   using entries in `commands.yaml`
3. **Differential testing:** Compare exit codes, JSON schema, error messages,
   and side effects

---

## Salesforce API Versions

Salesforce CLI commands interact with Salesforce orgs via REST and SOAP APIs.
Specific API versions depend on:
- Org configuration and capabilities
- Command requirements (e.g., `sf data query` uses `/services/data/query`)
- Org API version setting (typically v50.0+)

No single pinned API version applies to the entire CLI. Each command in
`commands.yaml` documents its specific endpoints.

---

## Future Updates

When upgrading the baseline:

1. **Document the new distribution:** Create a new entry in this directory
   or update `upstream-baseline.yaml` with the new version
2. **Verify plugins:** Confirm all bundled plugins ship together in the
   same release
3. **Verify sources:** Update commit hashes if source repositories have changed
4. **Update command inventory:** Add new commands to `commands.yaml` and mark
   removed commands as "no longer supported"
5. **Review licenses:** Check for new transitive dependencies and their licenses
6. **Announce the change:** Update the issue/PR that triggered the upgrade

---

## Related Documents

- [technical-architecture.md](../technical-architecture.md) - Verified source map,
  compatibility contract, and milestones
- [testing-strategy.md](../testing-strategy.md) - Test layers, oracle setup,
  and differential testing
- [development-setup.md](../development-setup.md) - Rust workspace setup,
  MSRV, lints, and CI
- [security-compliance.md](../security-compliance.md) - Credential handling,
  state management, and licensing policy
