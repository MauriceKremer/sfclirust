//! Read-only Salesforce project context.
//!
//! Project loading searches upward for `sfdx-project.json`, interprets the
//! fields needed to locate source, and retains the original JSON for callers.

pub mod metadata;

mod paths;
mod project_json;

use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

use paths::{discover_project, resolve_manifest_dir, resolve_package_directory};
use project_json::ProjectJson;

/// Failure while locating or interpreting a Salesforce project.
#[derive(Debug, Error)]
pub enum ProjectError {
    /// No project file was found from the supplied directory to the filesystem root.
    #[error("No sfdx-project.json found from {start} or its parent directories")]
    NotFound { start: PathBuf },

    /// A filesystem operation needed to load the project failed.
    #[error("Unable to access project path {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// The project file or its required structure is malformed.
    #[error("Invalid Salesforce project at {file}: {reason}")]
    Invalid { file: PathBuf, reason: String },

    /// The project uses a path or package layout this implementation will not load.
    #[error("Unsupported Salesforce project at {file}: {reason}")]
    Unsupported { file: PathBuf, reason: String },
}

impl ProjectError {
    /// Stable error code suitable for mapping to the CLI operational-error contract.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "PROJECT_NOT_FOUND",
            Self::Io { .. } => "PROJECT_IO_ERROR",
            Self::Invalid { .. } => "PROJECT_INVALID",
            Self::Unsupported { .. } => "PROJECT_UNSUPPORTED",
        }
    }

    fn invalid(file: &Path, reason: impl Into<String>) -> Self {
        Self::Invalid {
            file: file.to_path_buf(),
            reason: reason.into(),
        }
    }

    fn unsupported(file: &Path, reason: impl Into<String>) -> Self {
        Self::Unsupported {
            file: file.to_path_buf(),
            reason: reason.into(),
        }
    }

    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// A resolved Salesforce package directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDirectory {
    relative_path: PathBuf,
    source_path: PathBuf,
    is_default: bool,
}

impl PackageDirectory {
    /// Project-relative package path with separators normalized for this platform.
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Canonical source directory path.
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Whether this directory is the project's default package directory.
    pub fn is_default(&self) -> bool {
        self.is_default
    }
}

/// Loaded project context. Loading is read-only and never rewrites project files.
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    project_file: PathBuf,
    raw_config: Value,
    package_directories: Vec<PackageDirectory>,
    default_api_version: String,
    manifest_dir: PathBuf,
}

impl Project {
    /// Load the project containing `start_dir`, searching its ancestors for
    /// `sfdx-project.json`.
    ///
    /// Package paths must be relative, remain within the project root, and
    /// resolve to existing directories. Windows and POSIX separators in project
    /// paths are normalized on every platform.
    pub fn load(start_dir: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let start_dir = start_dir.as_ref();
        let start_dir =
            std::fs::canonicalize(start_dir).map_err(|error| ProjectError::io(start_dir, error))?;
        if !start_dir.is_dir() {
            return Err(ProjectError::invalid(
                &start_dir,
                "project discovery must start from a directory",
            ));
        }

        let (root, project_file) = discover_project(&start_dir)?;
        let parsed = ProjectJson::read(&project_file)?;
        let package_directories = parsed
            .package_directories
            .iter()
            .map(|package| {
                let relative_path = paths::normalize_relative_path(&project_file, &package.path)?;
                let source_path = resolve_package_directory(&root, &project_file, &relative_path)?;
                Ok(PackageDirectory {
                    relative_path,
                    source_path,
                    is_default: false,
                })
            })
            .collect::<Result<Vec<_>, ProjectError>>()?;
        let package_directories =
            apply_default_package(&project_file, package_directories, &parsed.package_defaults)?;
        let manifest_dir = resolve_manifest_dir(&root, &project_file)?;

        Ok(Self {
            root,
            project_file,
            raw_config: parsed.raw_config,
            package_directories,
            default_api_version: parsed.default_api_version,
            manifest_dir,
        })
    }

    /// Canonical project root containing `sfdx-project.json`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Canonical path to `sfdx-project.json`.
    pub fn project_file(&self) -> &Path {
        &self.project_file
    }

    /// Original project JSON, including fields this loader does not interpret.
    pub fn raw_config(&self) -> &Value {
        &self.raw_config
    }

    /// Resolved package directories in their declared order.
    pub fn package_directories(&self) -> &[PackageDirectory] {
        &self.package_directories
    }

    /// Source API version from the project, or the pinned upstream schema default.
    pub fn default_api_version(&self) -> &str {
        &self.default_api_version
    }

    /// Conventional project manifest directory (`<project>/manifest`).
    ///
    /// The directory need not exist until a workflow uses it.
    pub fn manifest_dir(&self) -> &Path {
        &self.manifest_dir
    }

    /// Resolved source directories in package declaration order.
    pub fn source_paths(&self) -> impl Iterator<Item = &Path> {
        self.package_directories
            .iter()
            .map(PackageDirectory::source_path)
    }
}

fn apply_default_package(
    project_file: &Path,
    mut package_directories: Vec<PackageDirectory>,
    package_defaults: &[Option<bool>],
) -> Result<Vec<PackageDirectory>, ProjectError> {
    if package_directories.len() == 1 {
        if package_defaults.first().copied().flatten() == Some(false) {
            return Err(ProjectError::invalid(
                project_file,
                "a single package directory cannot be marked default=false",
            ));
        }
        package_directories[0].is_default = true;
        return Ok(package_directories);
    }

    let defaults = package_defaults
        .iter()
        .enumerate()
        .filter_map(|(index, is_default)| (*is_default == Some(true)).then_some(index))
        .collect::<Vec<_>>();
    if defaults.len() != 1 {
        return Err(ProjectError::invalid(
            project_file,
            "multiple package directories require exactly one default=true entry",
        ));
    }
    package_directories[defaults[0]].is_default = true;
    Ok(package_directories)
}
