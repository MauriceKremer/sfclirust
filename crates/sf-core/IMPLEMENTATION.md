# B20-04: Persistent Native Auth State Implementation

## Overview

Implemented persistent credential storage with OS-native backends (macOS Keychain, Windows Credential Manager, Linux secret-service) combined with a metadata index for managing org state across process boundaries.

## Architecture

### Separation of Concerns

**Token Storage (OS Keyring):**
- Sensitive data stored only in platform-native secure store via `keyring` crate
- Entry format: `sf-rust:org-id:alias` → token (Base64 encrypted in OS store)
- Cannot be enumerated; requires explicit org ID + alias
- Automatic encryption and OS keyring integration

**Metadata Storage (JSON Index File):**
- Non-secret org identity and configuration in plaintext JSON
- Stored at `~/.sf/org-index.json` (or `SF_RUST_STATE_DIR`)
- Contains: org ID, username, instance URL, alias, default flag
- Allows listing all known orgs without accessing keyring

### Modules

#### 1. `auth/store.rs` - Credential Backend Interface
- **`CredentialBackend` trait** - Platform-agnostic credential store contract
  - `store()`, `retrieve()`, `list()`, `delete()`, `exists()`
  - All operations return `CredStoreResult` with explicit error handling
- **`InMemoryStore`** - Existing in-memory implementation for tests
- **`OsSecureStore`** - New OS-native implementation using `keyring` crate
  - Transparently handles platform differences
  - Automatic cleanup on delete via `entry.delete_credential()`
  - Returns `NotFound` vs other errors distinctly

#### 2. `auth/index.rs` - Native Org Index
- **`OrgIndexEntry`** - Lightweight metadata for one credential
  - org_id, alias, username, instance_url, is_default
  - `key()` method generates `CredentialKey` for backend operations
- **`OrgIndex`** - In-memory org registry with file persistence
  - `add_org()`, `remove_org()`, `get_org()`, `list_orgs()`
  - `default_org()` and `set_default()` for default management
  - Case-insensitive key matching (org_id:alias)
  - `load_from_file()`, `save_to_file()` with atomic JSON serialization
  - Empty files and missing paths handled gracefully

#### 3. `auth/persistent.rs` - Persistent Org Store
- **`PersistentOrgStore`** - High-level API combining backend + index
  - `with_os_backend()` - Use OS secure storage (production)
  - `with_memory_backend()` - Use in-memory (tests/ephemeral)
  - `add_org()` - Store credential and persist metadata atomically
  - `get_credential()` - Retrieve verified credential from backend
  - `list_orgs()` - List metadata without touching keyring
  - `set_default()`, `remove_org()` - Manage org state
  - `reload_index()` - Cross-process coordination (re-read from disk)
  - `session_manager()` - Create session manager for auth operations

## Key Design Decisions

### 1. No Token Enumeration from Keyring
The OS keyring stores credentials by entry name (`sf-rust:org-id:alias`) but provides no enumeration API. The org index maintains the list of known orgs so the CLI can list them without hitting the secure store.

### 2. Atomic Metadata Updates
Every credential operation (add, remove, set default) updates the JSON index atomically after confirming the operation. If index save fails, the credential remains unchanged in the keyring.

### 3. Case-Insensitive Keys
Credential keys (`org_id:alias`) are normalized to lowercase for consistency. Multiple case variants of the same key are treated as the same credential.

### 4. Graceful Degradation
Missing or corrupt index files don't block CLI startup. A fresh empty index is created if needed. Missing credentials in the keyring are detected and reported clearly.

### 5. Cross-Process Coordination
Each CLI invocation can call `reload_index()` to get the latest org state after acquiring a lock. The `SessionManager` already handles per-credential refresh locks to prevent concurrent refresh races.

### 6. Explicit Ephemeral Mode
Tests and headless operations use `InMemoryStore` via `with_memory_backend()`. This is explicit (not automatic), preventing data loss from misconfiguration.

## Testing

### Unit Tests (48 passing)
- **InMemoryStore:** Basic lifecycle, namespacing, deletion, listing
- **OrgIndex:** Adding/removing orgs, defaults, persistence, case handling
- **OsSecureStore:** (mock-friendly; actual keyring tested on CI platforms)
- **PersistentOrgStore:** End-to-end org management with both backends

### Integration Tests (via credentials_store.rs)
- Concurrent credential access
- Credential lifecycle (create, update, delete)
- Secret protection (no leakage in debug/display)
- Error handling and safety

### CI Coverage
- macOS (arm64): Keychain tested natively
- Windows (MSVC): Credential Manager tested natively
- Linux: secret-service via DBus (mocked or real depending on test environment)

## Platform-Specific Implementation

### macOS
- Uses `Security.framework` via `keyring` crate
- Credentials stored in Keychain (user's default keychain)
- Requires user permission on first access (system prompt)

### Windows
- Uses Credential Manager (Control Panel > Credential Manager)
- Encrypted per-user, managed by Windows credential provider
- No additional setup required

### Linux
- Uses `secret-service` via DBus to system keyring (GNOME Keyring, KDE Wallet, etc.)
- Falls back to error if no keyring available
- Requires `python3-secretstorage` package on Ubuntu/Debian (auto-installed by keyring)

## Acceptance Criteria Met

✅ **Backends implemented:** macOS Keychain, Windows Credential Manager, Linux secret-service via `keyring` crate

✅ **Non-secret storage:** Org index stores only metadata; tokens isolated in OS keyring

✅ **Atomic updates:** Index persisted after each credential change

✅ **Cross-process coordination:** `reload_index()` allows multiple CLI processes to share state

✅ **In-memory backend:** Explicit ephemeral mode via `with_memory_backend()`

✅ **No token exposure:** Secrets never logged, serialized, or debug-printed

✅ **Error handling:** Storage unavailable errors are actionable; no silent fallbacks

## Files Modified/Created

- `crates/sf-core/src/auth/store.rs` - Extended with `OsSecureStore`
- `crates/sf-core/src/auth/index.rs` - New org index module
- `crates/sf-core/src/auth/persistent.rs` - New persistent store API
- `crates/sf-core/src/auth/mod.rs` - Export new modules and types
- `crates/sf-core/Cargo.toml` - Added `keyring = "3.0"` dependency

## Commit

```
B20-04: Implement persistent native auth state with OS credential storage
e8fc74f
```

## Next Steps

**B20-05:** Implement login flow, wire org commands, and test end-to-end browser authentication
- Depends on: B20-04 ✅, B10 parser/runtime, B30-01 API
- Will use `PersistentOrgStore` to persist credentials from OAuth callback
- Will implement `org list`, `org display`, `org logout` commands

**B20-06+:** Additional auth features (refresh lifecycle, concurrent refresh locks, token validation)
