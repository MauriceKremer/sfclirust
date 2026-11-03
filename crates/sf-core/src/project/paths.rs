use std::fs;
use std::path::{Path, PathBuf};

use super::ProjectError;

const PROJECT_FILE_NAME: &str = "sfdx-project.json";
const MANIFEST_DIR_NAME: &str = "manifest";

pub(super) fn discover_project(start_dir: &Path) -> Result<(PathBuf, PathBuf), ProjectError> {
    for directory in start_dir.ancestors() {
        let project_file = directory.join(PROJECT_FILE_NAME);
        match fs::symlink_metadata(&project_file) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ProjectError::invalid(
                    &project_file,
                    "sfdx-project.json must not be a symbolic link",
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(ProjectError::invalid(
                    &project_file,
                    "sfdx-project.json must be a regular file",
                ));
            }
            Ok(_) => return Ok((directory.to_path_buf(), project_file)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(ProjectError::io(&project_file, error)),
        }
    }

    Err(ProjectError::NotFound {
        start: start_dir.to_path_buf(),
    })
}

pub(super) fn normalize_relative_path(
    project_file: &Path,
    value: &str,
) -> Result<PathBuf, ProjectError> {
    if value.is_empty() || value.contains('\0') {
        return Err(ProjectError::invalid(
            project_file,
            "package directory path must not be empty or contain NUL",
        ));
    }

    let normalized = value.replace('\\', "/");
    let first_component = normalized.split('/').next().unwrap_or_default();
    let is_windows_drive = first_component.as_bytes().get(1) == Some(&b':')
        && first_component
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    if normalized.starts_with('/') || is_windows_drive {
        return Err(ProjectError::unsupported(
            project_file,
            format!("package directory path must be relative: {value}"),
        ));
    }

    let mut result = PathBuf::new();
    for component in normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(ProjectError::unsupported(
                project_file,
                format!("package directory path escapes the project root: {value}"),
            ));
        }
        if component.contains(':') {
            return Err(ProjectError::unsupported(
                project_file,
                format!("package directory path is not portable across supported systems: {value}"),
            ));
        }
        result.push(component);
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Ok(result)
}

pub(super) fn resolve_package_directory(
    root: &Path,
    project_file: &Path,
    relative_path: &Path,
) -> Result<PathBuf, ProjectError> {
    let source_path = root.join(relative_path);
    let metadata = fs::metadata(&source_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProjectError::invalid(
                project_file,
                format!(
                    "package directory does not exist: {}",
                    relative_path.display()
                ),
            )
        } else {
            ProjectError::io(&source_path, error)
        }
    })?;
    if !metadata.is_dir() {
        return Err(ProjectError::invalid(
            project_file,
            format!(
                "package directory is not a directory: {}",
                relative_path.display()
            ),
        ));
    }

    let canonical_path =
        fs::canonicalize(&source_path).map_err(|error| ProjectError::io(&source_path, error))?;
    if !canonical_path.starts_with(root) {
        return Err(ProjectError::unsupported(
            project_file,
            format!(
                "package directory resolves outside the project root: {}",
                relative_path.display()
            ),
        ));
    }
    Ok(canonical_path)
}

pub(super) fn resolve_manifest_dir(
    root: &Path,
    project_file: &Path,
) -> Result<PathBuf, ProjectError> {
    let manifest_dir = root.join(MANIFEST_DIR_NAME);
    match fs::symlink_metadata(&manifest_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(manifest_dir),
        Err(error) => Err(ProjectError::io(&manifest_dir, error)),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let canonical_path = fs::canonicalize(&manifest_dir)
                .map_err(|error| ProjectError::io(&manifest_dir, error))?;
            if !canonical_path.starts_with(root) {
                return Err(ProjectError::unsupported(
                    project_file,
                    "manifest directory resolves outside the project root",
                ));
            }
            if !canonical_path.is_dir() {
                return Err(ProjectError::invalid(
                    project_file,
                    "manifest path is not a directory",
                ));
            }
            Ok(canonical_path)
        }
        Ok(metadata) if !metadata.is_dir() => Err(ProjectError::invalid(
            project_file,
            "manifest path is not a directory",
        )),
        Ok(_) => Ok(manifest_dir),
    }
}
