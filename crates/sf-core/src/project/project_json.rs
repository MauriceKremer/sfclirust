use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};

use super::ProjectError;

const MAX_PROJECT_JSON_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_SOURCE_API_VERSION: &str = "48.0";

#[derive(Debug)]
pub(super) struct ProjectJson {
    pub(super) raw_config: Value,
    pub(super) package_directories: Vec<PackageDirectoryConfig>,
    pub(super) package_defaults: Vec<Option<bool>>,
    pub(super) default_api_version: String,
}

#[derive(Debug)]
pub(super) struct PackageDirectoryConfig {
    pub(super) path: String,
}

impl ProjectJson {
    pub(super) fn read(file: &Path) -> Result<Self, ProjectError> {
        let handle = File::open(file).map_err(|error| ProjectError::io(file, error))?;
        let mut bytes = Vec::new();
        handle
            .take(MAX_PROJECT_JSON_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ProjectError::io(file, error))?;
        if bytes.len() as u64 > MAX_PROJECT_JSON_BYTES {
            return Err(ProjectError::invalid(
                file,
                format!("sfdx-project.json exceeds {MAX_PROJECT_JSON_BYTES} bytes"),
            ));
        }

        let raw_config: Value = serde_json::from_slice(&bytes).map_err(|error| {
            ProjectError::invalid(
                file,
                format!("sfdx-project.json is not valid JSON: {error}"),
            )
        })?;
        let object = raw_config.as_object().ok_or_else(|| {
            ProjectError::invalid(file, "sfdx-project.json must contain an object")
        })?;

        let (package_directories, package_defaults) = parse_package_directories(file, object)?;
        let default_api_version = match object.get("sourceApiVersion") {
            None => DEFAULT_SOURCE_API_VERSION.to_string(),
            Some(Value::String(version)) => version.clone(),
            Some(_) => {
                return Err(ProjectError::invalid(
                    file,
                    "sourceApiVersion must be a string",
                ));
            }
        };

        Ok(Self {
            raw_config,
            package_directories,
            package_defaults,
            default_api_version,
        })
    }
}

fn parse_package_directories(
    file: &Path,
    object: &Map<String, Value>,
) -> Result<(Vec<PackageDirectoryConfig>, Vec<Option<bool>>), ProjectError> {
    let value = object
        .get("packageDirectories")
        .ok_or_else(|| ProjectError::invalid(file, "packageDirectories is required"))?;
    let entries = value
        .as_array()
        .ok_or_else(|| ProjectError::invalid(file, "packageDirectories must be an array"))?;
    if entries.is_empty() {
        return Err(ProjectError::invalid(
            file,
            "packageDirectories must not be empty when specified",
        ));
    }

    let mut directories = Vec::with_capacity(entries.len());
    let mut defaults = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let entry = entry.as_object().ok_or_else(|| {
            ProjectError::invalid(
                file,
                format!("packageDirectories[{index}] must be an object"),
            )
        })?;
        let path = entry.get("path").and_then(Value::as_str).ok_or_else(|| {
            ProjectError::invalid(
                file,
                format!("packageDirectories[{index}].path must be a string"),
            )
        })?;
        let is_default = match entry.get("default") {
            None => None,
            Some(Value::Bool(value)) => Some(*value),
            Some(_) => {
                return Err(ProjectError::invalid(
                    file,
                    format!("packageDirectories[{index}].default must be a boolean"),
                ));
            }
        };
        directories.push(PackageDirectoryConfig {
            path: path.to_string(),
        });
        defaults.push(is_default);
    }

    Ok((directories, defaults))
}
