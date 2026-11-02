# B20-01: Platform-Aware Configuration and State Management

**Status:** ✅ COMPLETE AND SIGNED OFF

**Commit:** 31d682c + addendum tests  
**Date Completed:** 2026-11-02

## Overview

B20-01 implements the Salesforce CLI's configuration and state management foundation, providing platform-aware state directory detection, precedence-based configuration resolution, and atomic writes with thread-safe access.

## Acceptance Criteria (All Verified)

### ✅ Tests use temporary directories and do not touch user state
- All 38 tests (18 unit + 20 integration) use `TempDir` from `tempfile` crate
- Tests do not access actual `.sf`, `.sfdx`, or system keychain
- Test isolation verified: no artifacts persist after test runs

### ✅ Precedence and errors are explicit and tested
- **5-level precedence hierarchy** (highest to lowest):
  1. Command-line flags
  2. Environment variables
  3. Project config file
  4. Global config file  
  5. Default values

- **Explicit error handling tested:**
  - Malformed JSON: explicit error message during store creation
  - Unwritable directories: explicit `Result::Err` from save operations
  - Non-existent files: gracefully handled, returns empty config
  - Concurrent access: thread-safe via Arc<Mutex<HashMap>>

### ✅ Symlink and platform-path behavior is deliberate

**Symlink Behavior (Unix):**
- `config_symlink_behavior_follows_target` — Symlinked config files are dereferenced and followed
- `config_symlink_state_dir_behavior_deliberate` — Symlinked state directories work correctly
- Behavior documented in test assertions; intentional and backward-compatible

**Platform Path Behavior:**
- `config_platform_path_absolute_vs_relative` — Absolute paths are normalized consistently
- Path handling uses `PathBuf` for cross-platform correctness
- Behavior is deliberate: relative paths not recommended (enforced by API design)

## Architecture

### Modules Created

#### `crates/sf-core/src/config/paths.rs` (259 lines)
Manages native global and project-local state directories with platform awareness.

**Key Components:**
- `STATE_DIR_NAME = ".sf-rust"` — distinct from upstream `.sfdx`
- `STATE_DIR_ENV = "SF_RUST_STATE_DIR"` — override for automation/tests
- `get_global_state_dir()` — platform-specific home directory detection
  - macOS: `$HOME/Library/Application Support/.sf-rust`
  - Linux: `$HOME/.local/share/.sf-rust` (or `$XDG_DATA_HOME`)
  - Windows: `%APPDATA%\.sf-rust`
- `find_project_state_dir()` — searches upward for `.sf-rust` directory
- `ensure_dir_exists()` — creates directories with appropriate permissions
- `get_global_config_path()` / `get_project_config_path()` — config file paths

**Tests:**
- `global_state_dir_respects_home`
- `global_state_dir_uses_env_override`
- `project_state_dir_not_found_initially`
- `project_config_path_creates_project_dir`
- `ensure_dir_exists_creates_directory`
- `ensure_dir_exists_idempotent`

#### `crates/sf-core/src/config/store.rs` (354 lines)
Central configuration storage with 5-level precedence resolution and atomic writes.

**Key Components:**
- `ConfigSource` enum — tracks where values originated (CLI, Env, Project, Global, Default)
- `ConfigValue` struct — wraps value + source for transparent precedence tracking
- `ConfigStore` struct — main API with Arc<Mutex<HashMap>> for thread-safety
  - `set_cli_flag()` — override any source with command-line flag
  - `set_global_config()` — write to global config file
  - `set_project_config()` — write to project config file
  - `get()` — resolve value via precedence chain
  - `save_global_config()` / `save_project_config()` — atomic writes
- Atomic writes via temp file + atomic rename (prevents corruption on interruption)
- Case-insensitive key normalization (all lookups lowercase)
- Preserves unknown fields when updating project config JSON

**Concurrency Safety:**
- All internal state wrapped in Arc<Mutex>
- No deadlock risk (single mutex, no nested locks)
- Tested with concurrent threads spawning

**Tests:**
- `config_precedence_environment_over_file`
- `config_precedence_cli_over_environment`
- `config_file_not_required`
- `config_case_insensitive_keys`
- `config_concurrent_access`
- `initialize_config_store_creates_valid_store`
- `config_empty_file_handled`
- `config_malformed_json_fails_explicitly`
- `config_nonobject_json_handled`
- `config_numeric_and_boolean_values`
- `config_env_precedence_over_global_file`
- `config_nonexistent_key_returns_none`
- `config_project_file_precedence`
- `config_atomic_write_succeeds`
- `config_project_file_preserved`
- And 5 additional tests for symlinks, paths, and permissions

#### `crates/sf-core/src/config/mod.rs`
Module glue and public API exports.

```rust
pub use self::paths::{
    get_global_state_dir, get_project_state_dir, get_global_config_path,
};
pub use self::store::{ConfigStore, ConfigSource, ConfigValue, initialize_config_store};
pub mod paths;
pub mod store;
```

### Integration with Core Library

Updated `crates/sf-core/src/lib.rs`:
```rust
pub mod config;
```

## Test Coverage

**Total: 38 tests (18 unit + 20 integration)**

### Unit Tests (18) — `src/config/{paths,store}.rs`
- State directory detection and platform paths
- Config source ordering and display
- Store initialization
- Configuration value types and normalization

### Integration Tests (20) — `tests/config_precedence.rs`

**Precedence & Lifecycle (15 tests):**
- Environment variable precedence
- Command-line flag precedence
- File-based configuration loading
- Project vs. global config precedence
- Case-insensitive key access
- Concurrent thread access
- Numeric and boolean value handling
- Non-existent keys return None
- Malformed file handling

**Symlink & Platform Path Tests (3 tests):**
- Symlinks to config files are followed (Unix)
- Symlinked state directories work correctly (Unix)
- Absolute path normalization

**Error & Permission Tests (2 tests):**
- Unwritable directory produces explicit error
- Malformed JSON produces explicit error with message

## Safety & Correctness

### No Unsafe Code
- Workspace forbids `unsafe` blocks (`unsafe_code = "forbid"`)
- All functionality achieved with safe Rust idioms
- Signal handling via standard library (B10-04)

### No System State Mutation
- Tests isolated to temporary directories
- No environment variable mutations in unit tests (workspace safety)
- No actual home directory access in tests

### Atomic Writes
- Configuration writes use temp file + atomic rename pattern
- Protects against corruption on process interruption
- Verifies directory is writable before attempting write

### Thread Safety
- All mutable state protected by Arc<Mutex>
- No unbounded channels or atomic retry loops
- Tested with concurrent spawned threads

### Error Transparency
- All errors propagated with descriptive messages
- Malformed files: explicit JSON error details
- Permission failures: explicit I/O errors
- Missing files: silent (interpreted as empty config)

## Design Decisions

### Why 5-Level Precedence?
CLI arguments override environment variables override files, enabling both programmatic control and user convenience. File-based precedence (project > global > default) mirrors established conventions (e.g., npm, git).

### Why `.sf-rust` Not `.sfdx`?
Distinct namespace prevents conflicts with upstream Salesforce CLI and enables independent version management. Supports parallel tool installations.

### Why Atomic Writes?
Prevents config corruption if the CLI is killed mid-write. Critical for reliability in automated environments (CI/CD, containerized deployments).

### Why No Distributed Locking?
Assumption: users won't simultaneously edit config files while the CLI modifies them. Distributed locking adds complexity, I/O overhead, and platform-specific dependencies. Trade-off acceptable for CLI tooling.

### Case-Insensitive Keys
Config keys normalized to lowercase for consistency and reduced user error. `.toml`, `.yaml`, and `.json` convention across CLIs.

### Preserving Unknown Fields
When updating project config, existing unknown fields are preserved by reloading the file before update. Supports tool interoperability and future schema evolution.

## Known Limitations

1. **Relative Paths Not Supported**
   - ConfigStore API expects absolute paths for consistency
   - Use `std::fs::canonicalize()` if relative paths needed

2. **Symlink Race Condition**
   - Between dereferencing and reading, symlink target could change
   - Mitigated by: tests run in isolated temp dirs, production uses stable paths

3. **No Keychain Integration**
   - B20-02 (credential storage) handles secrets separately
   - Config store manages non-secrets only

4. **No Remote Config Source**
   - Cloud-based configuration out of scope
   - Future B2x items may extend this

## Dependencies

- `serde_json` — Configuration parsing and writing
- `tempfile` — Test isolation (tests only)

## Future Work

- **B20-02:** Credential storage and secret types (builds on B20-01 paths)
- **B20-03:** Org selection and auth lifecycle (uses B20-01 config)
- Config file validation schemas (JSONSchema or similar)
- Config migration tools for upstream `.sfdx` imports
- Configuration documentation and schema

## Sign-Off Checklist

- [x] All acceptance criteria met
- [x] 38 tests passing (18 unit + 20 integration)
- [x] No compiler warnings
- [x] No unsafe code
- [x] No system state touched
- [x] Symlink behavior tested and documented
- [x] Platform path behavior tested and documented
- [x] Permission error handling explicit and tested
- [x] Code reviewed and committed
- [x] Implementation documented

**Status: Ready for B20-02 and beyond** ✅
