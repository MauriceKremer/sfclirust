//! Configuration management module.
//!
//! Provides a unified interface for configuration with precedence resolution,
//! atomic writes, and concurrency safety.

pub mod paths;
pub mod store;

pub use paths::{
    ensure_dir_exists, find_project_state_dir, get_global_state_dir, global_config_path,
    project_config_path,
};
pub use store::{ConfigSource, ConfigStore, ConfigValue};

/// Initialize a configuration store with paths resolved.
///
/// Uses the standard paths unless overridden by environment variables.
pub fn initialize_config_store() -> Result<ConfigStore, String> {
    let global_path = global_config_path("config.json").ok();
    let project_path = project_config_path("project-config.json").ok();

    ConfigStore::new(global_path, project_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_config_store_succeeds() {
        let result = initialize_config_store();
        assert!(result.is_ok());
    }
}
