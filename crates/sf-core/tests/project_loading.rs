#![allow(clippy::expect_used, clippy::unwrap_used)]

use sf_core::project::{Project, ProjectError};
use std::fs;
use tempfile::TempDir;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/projects/");

fn project_with_fixture(fixture: &str) -> TempDir {
    let project = TempDir::new().expect("create project directory");
    let contents =
        fs::read_to_string(format!("{FIXTURES}{fixture}")).expect("read project fixture");
    fs::write(project.path().join("sfdx-project.json"), contents).expect("write project file");
    project
}

fn make_directory(root: &std::path::Path, relative: &str) {
    fs::create_dir_all(root.join(relative)).expect("create package directory");
}

#[test]
fn loads_project_context_from_a_nested_directory_without_rewriting_the_file() {
    let project = project_with_fixture("valid.json");
    make_directory(project.path(), "force-app");
    let nested = project.path().join("force-app/src");
    fs::create_dir_all(&nested).expect("create nested source directory");
    let project_file = project.path().join("sfdx-project.json");
    let original = fs::read(&project_file).expect("read original project file");
    let canonical_root = fs::canonicalize(project.path()).expect("canonicalize project root");

    let loaded = Project::load(&nested).expect("load project from nested directory");

    assert_eq!(loaded.root(), canonical_root);
    assert_eq!(
        loaded.project_file(),
        canonical_root.join("sfdx-project.json")
    );
    assert_eq!(loaded.default_api_version(), "63.0");
    assert_eq!(loaded.manifest_dir(), canonical_root.join("manifest"));
    assert_eq!(loaded.package_directories().len(), 1);
    assert!(loaded.package_directories()[0].is_default());
    let expected_source_path = canonical_root.join("force-app");
    assert_eq!(
        loaded.package_directories()[0].source_path(),
        expected_source_path
    );
    assert_eq!(
        loaded.source_paths().collect::<Vec<_>>(),
        vec![expected_source_path.as_path()]
    );
    assert_eq!(
        fs::read(&project_file).expect("read project file after loading"),
        original
    );
}

#[test]
fn preserves_unknown_project_fields_without_rewriting_them() {
    let project = project_with_fixture("unknown-field.json");
    make_directory(project.path(), "force-app");
    let project_file = project.path().join("sfdx-project.json");
    let original = fs::read(&project_file).expect("read original project file");

    let loaded = Project::load(project.path()).expect("load project with an unknown field");

    assert_eq!(
        loaded
            .raw_config()
            .get("customProjectData")
            .and_then(serde_json::Value::as_object)
            .and_then(|value| value.get("preserve"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        fs::read(&project_file).expect("read project file after loading"),
        original
    );
}

#[test]
fn reports_missing_project_context_with_the_cli_error_code() {
    let directory = TempDir::new().expect("create directory without a project");

    let error = Project::load(directory.path()).expect_err("project context should be missing");

    assert_eq!(error.code(), "PROJECT_NOT_FOUND");
    assert!(matches!(error, ProjectError::NotFound { .. }));
}

#[test]
fn reports_malformed_project_json_as_an_invalid_project() {
    let project = project_with_fixture("malformed.json");

    let error = Project::load(project.path()).expect_err("malformed JSON should fail");

    assert_eq!(error.code(), "PROJECT_INVALID");
}

#[test]
fn rejects_project_fields_that_do_not_match_the_pinned_schema_shape() {
    let project = TempDir::new().expect("create project directory");
    fs::write(
        project.path().join("sfdx-project.json"),
        r#"{"packageDirectories":"force-app"}"#,
    )
    .expect("write project with wrong field type");

    let error = Project::load(project.path()).expect_err("wrong field type should fail");

    assert_eq!(error.code(), "PROJECT_INVALID");
}

#[test]
fn requires_package_directories_as_defined_by_the_pinned_schema() {
    let project = TempDir::new().expect("create project directory");
    fs::write(
        project.path().join("sfdx-project.json"),
        r#"{"sourceApiVersion":"63.0"}"#,
    )
    .expect("write project without packageDirectories");

    let error = Project::load(project.path()).expect_err("packageDirectories is required");

    assert_eq!(error.code(), "PROJECT_INVALID");
}

#[test]
fn rejects_a_non_string_source_api_version() {
    let project = TempDir::new().expect("create project directory");
    make_directory(project.path(), "force-app");
    fs::write(
        project.path().join("sfdx-project.json"),
        r#"{"packageDirectories":[{"path":"force-app"}],"sourceApiVersion":63}"#,
    )
    .expect("write project with wrong API version type");

    let error = Project::load(project.path()).expect_err("sourceApiVersion must be a string");

    assert_eq!(error.code(), "PROJECT_INVALID");
}

#[test]
fn resolves_multiple_package_directories_and_their_default() {
    let project = project_with_fixture("multi-package.json");
    make_directory(project.path(), "packages/core");
    make_directory(project.path(), "packages/ui");

    let loaded = Project::load(project.path()).expect("load multi-package project");

    let canonical_root = fs::canonicalize(project.path()).expect("canonicalize project root");
    assert_eq!(loaded.package_directories().len(), 2);
    assert_eq!(
        loaded.package_directories()[0].source_path(),
        canonical_root.join("packages/core")
    );
    assert!(loaded.package_directories()[0].is_default());
    assert!(!loaded.package_directories()[1].is_default());
    assert_eq!(loaded.default_api_version(), "48.0");
    assert_eq!(
        loaded.raw_config()["packageDirectories"][0]["package"],
        "Core"
    );
}

#[test]
fn rejects_multiple_package_directories_without_exactly_one_default() {
    let project = TempDir::new().expect("create project directory");
    make_directory(project.path(), "packages/core");
    make_directory(project.path(), "packages/ui");
    fs::write(
        project.path().join("sfdx-project.json"),
        r#"{"packageDirectories":[{"path":"packages/core"},{"path":"packages/ui"}]}"#,
    )
    .expect("write project without a default package directory");

    let error = Project::load(project.path()).expect_err("default package must be unambiguous");

    assert_eq!(error.code(), "PROJECT_INVALID");
}

#[test]
fn normalizes_windows_separators_on_every_platform() {
    let project = project_with_fixture("windows-separators.json");
    make_directory(project.path(), "packages/windows");

    let loaded = Project::load(project.path()).expect("load project with Windows separators");

    let canonical_root = fs::canonicalize(project.path()).expect("canonicalize project root");
    assert_eq!(
        loaded.package_directories()[0].source_path(),
        canonical_root.join("packages/windows")
    );
}

#[test]
fn rejects_unsafe_package_paths_across_platform_conventions() {
    for path in [
        r"..\outside",
        "../outside",
        r"C:\outside",
        "C:/outside",
        r"\\server\share",
        "/outside",
    ] {
        let project = TempDir::new().expect("create project directory");
        let contents = format!(
            r#"{{"packageDirectories":[{{"path":{}}}]}}"#,
            serde_json::to_string(path).expect("encode package path")
        );
        fs::write(project.path().join("sfdx-project.json"), contents)
            .expect("write unsupported project");

        let error = Project::load(project.path()).expect_err("unsafe package path should fail");

        assert_eq!(error.code(), "PROJECT_UNSUPPORTED", "path: {path}");
    }
}

#[test]
fn rejects_a_package_directory_that_does_not_exist() {
    let project = project_with_fixture("valid.json");

    let error = Project::load(project.path()).expect_err("missing package directory should fail");

    assert_eq!(error.code(), "PROJECT_INVALID");
}

#[cfg(unix)]
#[test]
fn rejects_package_directories_that_symlink_outside_the_project() {
    use std::os::unix::fs::symlink;

    let project = TempDir::new().expect("create project directory");
    let outside = TempDir::new().expect("create external directory");
    symlink(outside.path(), project.path().join("external"))
        .expect("create package directory symlink");
    fs::write(
        project.path().join("sfdx-project.json"),
        r#"{"packageDirectories":[{"path":"external"}]}"#,
    )
    .expect("write project with external package directory");

    let error = Project::load(project.path()).expect_err("external symlink should fail");

    assert_eq!(error.code(), "PROJECT_UNSUPPORTED");
}

#[cfg(unix)]
#[test]
fn rejects_a_manifest_directory_that_symlinks_outside_the_project() {
    use std::os::unix::fs::symlink;

    let project = project_with_fixture("valid.json");
    let outside = TempDir::new().expect("create external manifest directory");
    make_directory(project.path(), "force-app");
    symlink(outside.path(), project.path().join("manifest"))
        .expect("create manifest directory symlink");

    let error = Project::load(project.path()).expect_err("external manifest symlink should fail");

    assert_eq!(error.code(), "PROJECT_UNSUPPORTED");
}
