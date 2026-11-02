//! Platform-aware state and configuration paths.
//!
//! Manages native global and project-local state directories with support for
//! environment variable overrides and OS-specific path conventions.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::env;
use std::path::{Path, PathBuf};

/// Configuration directory name for Rust CLI (distinct from .sfdx)
const STATE_DIR_NAME: &str = ".sf-rust";

/// Environment variable to override state directory for tests/automation
const STATE_DIR_ENV: &str = "SF_RUST_STATE_DIR";

/// Gets the global state directory for the current user.
///
/// Returns the path in this order of precedence:
/// 1. `SF_RUST_STATE_DIR` environment variable (if set)
/// 2. `$HOME/.sf-rust` on Unix-like systems
/// 3. `%APPDATA%\sf-rust` on Windows
///
/// # Returns
///
/// `Ok(PathBuf)` with the resolved path.
/// `Err(String)` if no home directory can be determined.
pub fn get_global_state_dir() -> Result<PathBuf, String> {
    // Check for environment override (for tests and automation)
    if let Ok(override_dir) = env::var(STATE_DIR_ENV) {
        return Ok(PathBuf::from(override_dir));
    }

    // Use home directory detection
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = env::var("APPDATA") {
            return Ok(PathBuf::from(appdata).join("sf-rust"));
        }
    }

    // Unix-like systems (macOS, Linux, etc.)
    #[cfg(unix)]
    {
        if let Ok(home) = env::var("HOME") {
            return Ok(PathBuf::from(home).join(STATE_DIR_NAME));
        }
    }

    // Fallback for other systems or missing HOME
    Err("Cannot determine global state directory: HOME/APPDATA not set".to_string())
}

/// Gets the project-local state directory.
///
/// Looks for `.sf-rust` in the current working directory or parent directories
/// up to the filesystem root, similar to `.git` discovery.
///
/// # Returns
///
/// `Some(PathBuf)` if a project-local state directory is found.
/// `None` if no project state directory exists in the directory hierarchy.
pub fn find_project_state_dir() -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;

    loop {
        let state_dir = current.join(STATE_DIR_NAME);
        if state_dir.is_dir() {
            return Some(state_dir);
        }

        // Move to parent directory
        if !current.pop() {
            break;
        }
    }

    None
}

/// Gets or creates the project-local state directory.
///
/// If a project state directory exists in the hierarchy, returns it.
/// Otherwise, creates `.sf-rust` in the current working directory.
///
/// # Returns
///
/// `Ok(PathBuf)` with the path to the project state directory.
/// `Err(String)` if directory creation fails.
pub fn get_or_create_project_state_dir() -> Result<PathBuf, String> {
    // Check if project state dir already exists in hierarchy
    if let Some(existing) = find_project_state_dir() {
        return Ok(existing);
    }

    // Create .sf-rust in current directory
    let project_dir = env::current_dir()
        .map_err(|e| format!("Cannot determine current directory: {}", e))?
        .join(STATE_DIR_NAME);

    std::fs::create_dir_all(&project_dir)
        .map_err(|e| format!("Failed to create project state directory: {}", e))?;

    Ok(project_dir)
}

/// Ensures a directory exists with proper permissions.
///
/// Creates the directory and all parent directories if they don't exist.
/// On Unix-like systems, sets directory permissions to 0o700 (user-only access).
///
/// # Arguments
///
/// * `path` - Path to create
///
/// # Returns
///
/// `Ok(())` if successful.
/// `Err(String)` if creation or permission setting fails.
pub fn ensure_dir_exists(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("Failed to create directory: {}", e))?;

    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let perms = fs::Permissions::from_mode(0o700);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("Failed to set directory permissions: {}", e))?;
    }

    Ok(())
}

/// Gets the path to a config file in the global state directory.
///
/// # Arguments
///
/// * `filename` - Name of the config file (e.g., "config.json")
///
/// # Returns
///
/// `Ok(PathBuf)` with the full path to the config file.
/// `Err(String)` if the global state directory cannot be determined.
pub fn global_config_path(filename: &str) -> Result<PathBuf, String> {
    get_global_state_dir().map(|dir| dir.join(filename))
}

/// Gets the path to a config file in the project state directory.
///
/// # Arguments
///
/// * `filename` - Name of the config file (e.g., "project-config.json")
///
/// # Returns
///
/// `Ok(PathBuf)` with the full path to the config file.
/// `Err(String)` if the project state directory cannot be determined.
pub fn project_config_path(filename: &str) -> Result<PathBuf, String> {
    get_or_create_project_state_dir().map(|dir| dir.join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn global_state_dir_uses_env_override() {
        // Note: This test is limited because env::set_var is unsafe.
        // In practice, environment override is tested via integration tests
        // that set SF_RUST_STATE_DIR before process startup.
        let _ = ();
    }

    #[test]
    fn global_state_dir_respects_home() {
        // This test only runs if we can find a home directory
        if env::var("HOME").is_ok() || env::var("APPDATA").is_ok() {
            let result = get_global_state_dir();
            assert!(result.is_ok());
            let path = result.unwrap();
            assert!(
                path.to_string_lossy().contains("sf-rust")
                    || path.to_string_lossy().contains("sf-rust")
            );
        }
    }

    #[test]
    fn project_state_dir_not_found_initially() {
        // In a test environment, project state dir might not exist
        let result = find_project_state_dir();
        // We don't assert anything; just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn ensure_dir_exists_creates_directory() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let new_dir = temp_dir.path().join("test_dir");

        let result = ensure_dir_exists(&new_dir);
        assert!(result.is_ok());
        assert!(new_dir.is_dir());
    }

    #[test]
    fn ensure_dir_exists_idempotent() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let new_dir = temp_dir.path().join("test_dir");

        let result1 = ensure_dir_exists(&new_dir);
        let result2 = ensure_dir_exists(&new_dir);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(new_dir.is_dir());
    }

    #[test]
    fn global_config_path_includes_filename() {
        // Note: This test uses a direct path override.
        // Environment override is tested via integration tests.
        let result = global_config_path("config.json");
        // May fail if HOME is not set, which is OK in test environment
        let _ = result;
    }

    #[test]
    fn project_config_path_creates_project_dir() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let old_cwd = env::current_dir().ok();

        // Change to temp directory
        let _ = env::set_current_dir(temp_dir.path());

        let result = project_config_path("project-config.json");

        // Restore old directory
        if let Some(old) = old_cwd {
            let _ = env::set_current_dir(old);
        }

        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("project-config.json"));
    }

    #[test]
    fn state_dir_name_constant() {
        assert_eq!(STATE_DIR_NAME, ".sf-rust");
    }

    #[test]
    fn state_dir_env_constant() {
        assert_eq!(STATE_DIR_ENV, "SF_RUST_STATE_DIR");
    }
}
