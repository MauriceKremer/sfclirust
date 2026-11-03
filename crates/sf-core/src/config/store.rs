//! Configuration storage with precedence, atomic writes, and concurrency safety.
//!
//! Manages configuration with explicit precedence:
//! 1. Command-line flags
//! 2. Environment variables
//! 3. Project config file
//! 4. Global config file
//! 5. Defaults

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

/// Represents a configuration preference with its source.
#[derive(Debug, Clone)]
pub struct ConfigValue {
    /// The actual value
    pub value: String,
    /// Where this value came from (for debugging and precedence)
    pub source: ConfigSource,
}

/// Tracks where a configuration value originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigSource {
    /// Default value (lowest precedence)
    Default,
    /// Global user config file
    GlobalConfig,
    /// Project config file
    ProjectConfig,
    /// Environment variable
    Environment,
    /// Command-line flag (highest precedence)
    CommandLine,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::Default => write!(f, "default"),
            ConfigSource::GlobalConfig => write!(f, "global config"),
            ConfigSource::ProjectConfig => write!(f, "project config"),
            ConfigSource::Environment => write!(f, "environment"),
            ConfigSource::CommandLine => write!(f, "command line"),
        }
    }
}

/// Configuration store with precedence-based lookups.
#[derive(Debug)]
pub struct ConfigStore {
    /// Global configuration (loaded from file)
    global_config: Arc<Mutex<HashMap<String, String>>>,
    /// Project configuration (loaded from file)
    project_config: Arc<Mutex<HashMap<String, String>>>,
    /// Environment variable overrides
    env_config: HashMap<String, String>,
    /// Command-line flag overrides
    cli_config: Arc<Mutex<HashMap<String, String>>>,
    /// Default values
    defaults: HashMap<String, String>,
    /// Paths for persistence
    global_config_path: Option<PathBuf>,
    project_config_path: Option<PathBuf>,
}

impl ConfigStore {
    /// Create a new configuration store with optional paths.
    pub fn new(
        global_path: Option<PathBuf>,
        project_path: Option<PathBuf>,
    ) -> Result<Self, String> {
        let global_config = if let Some(ref path) = global_path {
            Self::load_config(path)?
        } else {
            HashMap::new()
        };

        let project_config = if let Some(ref path) = project_path {
            Self::load_config(path)?
        } else {
            HashMap::new()
        };

        // Collect environment variables with "SF_" prefix
        let env_config = Self::collect_env_config();

        Ok(Self {
            global_config: Arc::new(Mutex::new(global_config)),
            project_config: Arc::new(Mutex::new(project_config)),
            env_config,
            cli_config: Arc::new(Mutex::new(HashMap::new())),
            defaults: Self::default_config(),
            global_config_path: global_path,
            project_config_path: project_path,
        })
    }

    /// Load configuration from a JSON file.
    fn load_config(path: &Path) -> Result<HashMap<String, String>, String> {
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;

        let json: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Invalid JSON in config file: {}", e))?;

        let mut config = HashMap::new();
        if let Some(obj) = json.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    config.insert(k.clone(), s.to_string());
                } else if v.is_number() || v.is_boolean() {
                    config.insert(k.clone(), v.to_string());
                }
            }
        }

        Ok(config)
    }

    /// Collect environment variables starting with "SF_".
    fn collect_env_config() -> HashMap<String, String> {
        let mut config = HashMap::new();
        for (key, value) in std::env::vars() {
            if let Some(stripped) = key.strip_prefix("SF_") {
                let config_key = stripped.to_lowercase();
                config.insert(config_key, value);
            }
        }
        config
    }

    /// Default configuration values.
    fn default_config() -> HashMap<String, String> {
        let defaults = HashMap::new();
        // Add default values as needed
        defaults
    }

    /// Get a configuration value with precedence resolution.
    ///
    /// Returns the value with the highest precedence or None if not found.
    pub fn get(&self, key: &str) -> Option<ConfigValue> {
        let key_lower = key.to_lowercase();

        // Check in order of precedence (highest to lowest)

        // 1. Command-line flags
        if let Ok(cli) = self.cli_config.lock()
            && let Some(value) = cli.get(&key_lower)
        {
            return Some(ConfigValue {
                value: value.clone(),
                source: ConfigSource::CommandLine,
            });
        }

        // 2. Environment variables
        if let Some(value) = self.env_config.get(&key_lower) {
            return Some(ConfigValue {
                value: value.clone(),
                source: ConfigSource::Environment,
            });
        }

        // 3. Project config
        if let Ok(project) = self.project_config.lock()
            && let Some(value) = project.get(&key_lower)
        {
            return Some(ConfigValue {
                value: value.clone(),
                source: ConfigSource::ProjectConfig,
            });
        }

        // 4. Global config
        if let Ok(global) = self.global_config.lock()
            && let Some(value) = global.get(&key_lower)
        {
            return Some(ConfigValue {
                value: value.clone(),
                source: ConfigSource::GlobalConfig,
            });
        }

        // 5. Defaults
        if let Some(value) = self.defaults.get(&key_lower) {
            return Some(ConfigValue {
                value: value.clone(),
                source: ConfigSource::Default,
            });
        }

        None
    }

    /// Set a command-line flag value (highest precedence).
    pub fn set_cli_flag(&self, key: String, value: String) {
        if let Ok(mut cli) = self.cli_config.lock() {
            cli.insert(key.to_lowercase(), value);
        }
    }

    /// Save global configuration atomically.
    ///
    /// Uses a temporary file and atomic rename to ensure consistency.
    pub fn save_global_config(&self) -> Result<(), String> {
        let path = self
            .global_config_path
            .as_ref()
            .ok_or("Global config path not set")?;

        // Prepare parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let config = self
            .global_config
            .lock()
            .map_err(|e| format!("Failed to acquire lock: {}", e))?;

        let mut json_obj = json!({});
        if let Some(obj) = json_obj.as_object_mut() {
            for (k, v) in config.iter() {
                obj.insert(k.clone(), Value::String(v.clone()));
            }
        }

        let json_str = serde_json::to_string_pretty(&json_obj)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        // Write to temporary file
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)
            .map_err(|e| format!("Failed to create temp file: {}", e))?;

        file.write_all(json_str.as_bytes())
            .map_err(|e| format!("Failed to write config: {}", e))?;

        file.sync_all()
            .map_err(|e| format!("Failed to sync config file: {}", e))?;

        // Atomic rename
        fs::rename(&temp_path, path).map_err(|e| format!("Failed to rename config file: {}", e))?;

        Ok(())
    }

    /// Save project configuration atomically.
    pub fn save_project_config(&self) -> Result<(), String> {
        let path = self
            .project_config_path
            .as_ref()
            .ok_or("Project config path not set")?;

        // Preserve unknown fields if they exist
        let mut existing_json = if path.exists() {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("Failed to read existing config: {}", e))?;
            serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };

        // Update known fields
        let config = self
            .project_config
            .lock()
            .map_err(|e| format!("Failed to acquire lock: {}", e))?;

        if let Some(obj) = existing_json.as_object_mut() {
            for (k, v) in config.iter() {
                obj.insert(k.clone(), Value::String(v.clone()));
            }
        }

        let json_str = serde_json::to_string_pretty(&existing_json)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        // Write to temporary file
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)
            .map_err(|e| format!("Failed to create temp file: {}", e))?;

        file.write_all(json_str.as_bytes())
            .map_err(|e| format!("Failed to write config: {}", e))?;

        file.sync_all()
            .map_err(|e| format!("Failed to sync config file: {}", e))?;

        // Atomic rename
        fs::rename(&temp_path, path).map_err(|e| format!("Failed to rename config file: {}", e))?;

        Ok(())
    }

    /// Set a project config value.
    pub fn set_project_config(&self, key: String, value: String) -> Result<(), String> {
        if let Ok(mut project) = self.project_config.lock() {
            project.insert(key, value);
            Ok(())
        } else {
            Err("Failed to acquire lock on project config".to_string())
        }
    }

    /// Set a global config value.
    pub fn set_global_config(&self, key: String, value: String) -> Result<(), String> {
        if let Ok(mut global) = self.global_config.lock() {
            global.insert(key, value);
            Ok(())
        } else {
            Err("Failed to acquire lock on global config".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_source_display() {
        assert_eq!(ConfigSource::Default.to_string(), "default");
        assert_eq!(ConfigSource::GlobalConfig.to_string(), "global config");
        assert_eq!(ConfigSource::ProjectConfig.to_string(), "project config");
        assert_eq!(ConfigSource::Environment.to_string(), "environment");
        assert_eq!(ConfigSource::CommandLine.to_string(), "command line");
    }

    #[test]
    fn config_source_ordering() {
        assert!(ConfigSource::Default < ConfigSource::GlobalConfig);
        assert!(ConfigSource::GlobalConfig < ConfigSource::ProjectConfig);
        assert!(ConfigSource::ProjectConfig < ConfigSource::Environment);
        assert!(ConfigSource::Environment < ConfigSource::CommandLine);
    }

    #[test]
    fn config_store_creation() {
        let store = ConfigStore::new(None, None);
        assert!(store.is_ok());
    }

    #[test]
    fn cli_flag_takes_precedence() {
        let store = ConfigStore::new(None, None).expect("create store");
        store.set_cli_flag("test_key".to_string(), "cli_value".to_string());

        let value = store.get("test_key");
        assert!(value.is_some());
        let cv = value.unwrap();
        assert_eq!(cv.value, "cli_value");
        assert_eq!(cv.source, ConfigSource::CommandLine);
    }

    #[test]
    fn set_project_config_succeeds() {
        let store = ConfigStore::new(None, None).expect("create store");
        let result = store.set_project_config("key".to_string(), "value".to_string());
        assert!(result.is_ok());

        let value = store.get("key");
        assert!(value.is_some());
        assert_eq!(value.unwrap().value, "value");
    }

    #[test]
    fn set_global_config_succeeds() {
        let store = ConfigStore::new(None, None).expect("create store");
        let result = store.set_global_config("key".to_string(), "value".to_string());
        assert!(result.is_ok());

        let value = store.get("key");
        assert!(value.is_some());
        assert_eq!(value.unwrap().value, "value");
    }

    #[test]
    fn default_config_precedence() {
        let store = ConfigStore::new(None, None).expect("create store");
        // Defaults are empty, so this should return None
        let value = store.get("nonexistent");
        assert!(value.is_none());
    }

    #[test]
    fn case_insensitive_keys() {
        let store = ConfigStore::new(None, None).expect("create store");
        store.set_cli_flag("TestKey".to_string(), "value".to_string());

        // Should work with different casing
        let value1 = store.get("testkey");
        let value2 = store.get("TESTKEY");
        let value3 = store.get("TestKey");

        assert_eq!(value1.map(|v| v.value), Some("value".to_string()));
        assert_eq!(value2.map(|v| v.value), Some("value".to_string()));
        assert_eq!(value3.map(|v| v.value), Some("value".to_string()));
    }
}
