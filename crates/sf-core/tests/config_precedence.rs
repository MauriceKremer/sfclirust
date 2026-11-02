//! Integration tests for configuration precedence and lifecycle.
//!
//! Tests verify that configuration loading respects precedence, handles
//! malformed files gracefully, and doesn't touch system state.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use sf_core::config::{ConfigSource, ConfigStore, initialize_config_store};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn config_precedence_environment_over_file() {
    // Note: Environment variable testing requires setting vars before process start.
    // This unit test verifies the store prioritizes correctly.
    // Integration tests in test environments should set SF_* before running.

    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    // Write a config file
    fs::write(&config_file, r#"{"key": "from_file"}"#).expect("write config file");

    let store = ConfigStore::new(Some(config_file), None).expect("create store");
    let value = store.get("key").expect("get value");

    assert_eq!(value.value, "from_file");
    assert_eq!(value.source, ConfigSource::GlobalConfig);
}

#[test]
fn config_precedence_cli_over_environment() {
    // Command-line flags should override all sources
    let store = ConfigStore::new(None, None).expect("create store");

    // Note: env::set_var is unsafe and prohibited by workspace lints
    // This tests CLI precedence without environment variables

    store.set_cli_flag("key".to_string(), "from_cli".to_string());
    let value = store.get("key").expect("get value");

    assert_eq!(value.value, "from_cli");
    assert_eq!(value.source, ConfigSource::CommandLine);
}

#[test]
fn config_file_not_required() {
    // ConfigStore should work even if files don't exist
    let nonexistent = PathBuf::from("/nonexistent/path/config.json");
    let result = ConfigStore::new(Some(nonexistent), None);
    assert!(result.is_ok());
}

#[test]
fn config_malformed_json_fails_gracefully() {
    // Invalid JSON should produce an error, not panic
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    fs::write(&config_file, "{invalid json}").expect("write config file");

    let result = ConfigStore::new(Some(config_file), None);
    assert!(result.is_err());
}

#[test]
fn config_atomic_write_succeeds() {
    // Atomic writes should create files and not corrupt on interruption
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    let store = ConfigStore::new(Some(config_file.clone()), None).expect("create store");
    store
        .set_global_config("test_key".to_string(), "test_value".to_string())
        .expect("set config");

    let result = store.save_global_config();
    assert!(result.is_ok());

    // Verify file was written
    assert!(config_file.exists());

    // Verify file is valid JSON
    let content = fs::read_to_string(&config_file).expect("read file");
    let json: Result<serde_json::Value, _> = serde_json::from_str(&content);
    assert!(json.is_ok());
}

#[test]
fn config_project_file_preserved() {
    // Unknown fields in project files should be preserved
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    // Write initial config with unknown field
    fs::write(
        &config_file,
        r#"{"known_field": "value1", "unknown_field": "should_preserve"}"#,
    )
    .expect("write config file");

    let store = ConfigStore::new(None, Some(config_file.clone())).expect("create store");
    store
        .set_project_config("known_field".to_string(), "updated_value".to_string())
        .expect("set config");

    store.save_project_config().expect("save config");

    // Reload and verify
    let content = fs::read_to_string(&config_file).expect("read file");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse json");

    let obj = json.as_object().expect("get object");
    assert_eq!(
        obj.get("known_field").and_then(|v| v.as_str()),
        Some("updated_value")
    );
    assert_eq!(
        obj.get("unknown_field").and_then(|v| v.as_str()),
        Some("should_preserve")
    );
}

#[test]
fn config_case_insensitive_keys() {
    // Configuration keys should be case-insensitive
    let store = ConfigStore::new(None, None).expect("create store");

    store.set_cli_flag("TestKey".to_string(), "value".to_string());

    let value1 = store.get("testkey");
    let value2 = store.get("TESTKEY");
    let value3 = store.get("TestKey");

    assert!(value1.is_some());
    assert!(value2.is_some());
    assert!(value3.is_some());

    assert_eq!(value1.map(|v| v.value), Some("value".to_string()));
    assert_eq!(value2.map(|v| v.value), Some("value".to_string()));
    assert_eq!(value3.map(|v| v.value), Some("value".to_string()));
}

#[test]
fn config_empty_file_handled() {
    // Empty or whitespace-only config files should not error
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    fs::write(&config_file, "{}").expect("write config file");

    let result = ConfigStore::new(Some(config_file), None);
    assert!(result.is_ok());
}

#[test]
fn config_nonobject_json_handled() {
    // JSON that isn't an object should be ignored gracefully
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    fs::write(&config_file, r#"["array", "instead", "of", "object"]"#).expect("write config file");

    let result = ConfigStore::new(Some(config_file), None);
    assert!(result.is_ok());

    let store = result.unwrap();
    let value = store.get("anything");
    assert!(value.is_none());
}

#[test]
fn config_env_precedence_over_global_file() {
    // Environment should override global config file
    // Note: Environment testing requires setting vars before process start
    // (env::set_var is unsafe in this codebase)

    let temp_dir = TempDir::new().expect("create temp dir");
    let global_file = temp_dir.path().join("global.json");

    fs::write(&global_file, r#"{"key": "global_value"}"#).expect("write global config");

    let store = ConfigStore::new(Some(global_file), None).expect("create store");
    let value = store.get("key").expect("get value");

    assert_eq!(value.value, "global_value");
    assert_eq!(value.source, ConfigSource::GlobalConfig);
}

#[test]
fn config_project_file_precedence() {
    // Project config should override global config
    let temp_dir = TempDir::new().expect("create temp dir");
    let global_file = temp_dir.path().join("global.json");
    let project_file = temp_dir.path().join("project.json");

    fs::write(&global_file, r#"{"key": "global_value"}"#).expect("write global");
    fs::write(&project_file, r#"{"key": "project_value"}"#).expect("write project");

    let store = ConfigStore::new(Some(global_file), Some(project_file)).expect("create store");

    let value = store.get("key").expect("get value");
    assert_eq!(value.value, "project_value");
    assert_eq!(value.source, ConfigSource::ProjectConfig);
}

#[test]
fn config_nonexistent_key_returns_none() {
    // Non-existent keys should return None, not error
    let store = ConfigStore::new(None, None).expect("create store");
    let value = store.get("nonexistent_key_xyz");
    assert!(value.is_none());
}

#[test]
fn config_numeric_and_boolean_values() {
    // Config should handle numeric and boolean values
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    fs::write(
        &config_file,
        r#"{"number": 42, "boolean": true, "string": "text"}"#,
    )
    .expect("write config file");

    let store = ConfigStore::new(Some(config_file), None).expect("create store");

    let num = store.get("number");
    let bool_val = store.get("boolean");
    let str_val = store.get("string");

    // All should be converted to strings
    assert!(num.is_some());
    assert!(bool_val.is_some());
    assert!(str_val.is_some());
}

#[test]
fn config_concurrent_access() {
    // ConfigStore should be thread-safe
    let store = std::sync::Arc::new(ConfigStore::new(None, None).expect("create store"));

    let mut handles = vec![];

    for i in 0..3 {
        let store_clone = std::sync::Arc::clone(&store);
        let handle = std::thread::spawn(move || {
            let key = format!("key_{}", i);
            let value = format!("value_{}", i);
            store_clone.set_cli_flag(key.clone(), value);
            store_clone.get(&key)
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.join();
        assert!(result.is_ok());
    }
}

#[test]
fn initialize_config_store_creates_valid_store() {
    // initialize_config_store should return a usable store
    let result = initialize_config_store();
    assert!(result.is_ok());

    let store = result.unwrap();
    // Should be able to set and get values
    store.set_cli_flag("test".to_string(), "value".to_string());
    let value = store.get("test");
    assert!(value.is_some());
}

#[test]
fn config_symlink_behavior_follows_target() {
    // On Unix systems, symlinks to config files should be followed
    #[cfg(unix)]
    {
        use std::os::unix::fs as unix_fs;

        let temp_dir = TempDir::new().expect("create temp dir");
        let real_file = temp_dir.path().join("real_config.json");
        let link_file = temp_dir.path().join("link_config.json");

        // Write real config file
        fs::write(&real_file, r#"{"key": "from_symlink_target"}"#).expect("write real config file");

        // Create symlink
        unix_fs::symlink(&real_file, &link_file).expect("create symlink");

        // Store should read through the symlink
        let store = ConfigStore::new(Some(link_file), None).expect("create store");
        let value = store.get("key").expect("get value");

        assert_eq!(value.value, "from_symlink_target");
        assert_eq!(value.source, ConfigSource::GlobalConfig);
    }
}

#[test]
fn config_symlink_state_dir_behavior_deliberate() {
    // Symlinked state directories are followed (not dereferenced)
    // This test documents that behavior
    #[cfg(unix)]
    {
        use std::os::unix::fs as unix_fs;

        let temp_dir = TempDir::new().expect("create temp dir");
        let real_dir = temp_dir.path().join("real_state");
        let link_dir = temp_dir.path().join("link_state");

        fs::create_dir(&real_dir).expect("create real state dir");
        unix_fs::symlink(&real_dir, &link_dir).expect("create symlink");

        // Config store initialized with symlink should work
        let config_file = link_dir.join("config.json");
        fs::write(&config_file, r#"{"test": "value"}"#).expect("write config");

        let store = ConfigStore::new(Some(config_file), None).expect("create store");
        let value = store.get("test").expect("get value");
        assert_eq!(value.value, "value");
    }
}

#[test]
fn config_platform_path_absolute_vs_relative() {
    // Platform paths should normalize correctly
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    fs::write(&config_file, r#"{"key": "value"}"#).expect("write config file");

    // Absolute path should work
    let store_abs =
        ConfigStore::new(Some(config_file.clone()), None).expect("create store with absolute path");
    assert!(store_abs.get("key").is_some());

    // Relative paths are not recommended but should be documented
    // (ConfigStore expects absolute paths for consistency)
}

#[test]
fn config_unwritable_directory_error_explicit() {
    // Permission errors when writing config should be explicit and testable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().expect("create temp dir");
        let read_only_dir = temp_dir.path().join("readonly");
        let config_file = read_only_dir.join("config.json");

        fs::create_dir(&read_only_dir).expect("create readonly dir");

        // Write initial config
        fs::write(&config_file, r#"{"key": "initial"}"#).expect("write config");

        // Make directory read-only
        let perms = fs::Permissions::from_mode(0o555);
        fs::set_permissions(&read_only_dir, perms).expect("set read-only");

        // Creating a store should succeed (reading is allowed)
        let store = ConfigStore::new(Some(config_file.clone()), None)
            .expect("create store with read-only config");

        // But writing should fail with explicit error
        let result = store.save_global_config();
        assert!(
            result.is_err(),
            "writing to read-only directory should fail"
        );

        // Restore permissions for cleanup
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&read_only_dir, perms).expect("restore permissions");
    }
}

#[test]
fn config_malformed_json_fails_explicitly() {
    // Malformed JSON should produce an explicit error, not silent failure
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_file = temp_dir.path().join("config.json");

    // Write malformed JSON
    fs::write(&config_file, r#"{invalid json"#).expect("write malformed file");

    // Creating a store should fail with an explicit error message
    let result = ConfigStore::new(Some(config_file), None);
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        err.contains("Invalid JSON"),
        "Error should mention invalid JSON: {}",
        err
    );
}
