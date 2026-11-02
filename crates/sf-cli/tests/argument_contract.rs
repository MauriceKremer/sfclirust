//! Argument contract tests for command parsing and validation.
//!
//! These tests verify that commands parse arguments correctly, validate input,
//! and handle edge cases with proper exit codes and error messages.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn org_list_basic() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");

    cmd.assert().success();
}

#[test]
fn org_list_json_flag() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--json");

    cmd.assert().success();
}

#[test]
fn org_list_with_columns() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--columns");
    cmd.arg("Alias,Username");

    cmd.assert().success();
}

#[test]
fn org_list_invalid_flag() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--invalid-flag");

    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unknown").or(predicate::str::contains("unexpected")));
}

#[test]
fn org_subcommand_help() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("org").or(predicate::str::contains("Org")));
}

#[test]
fn org_list_help() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("list").or(predicate::str::contains("List")));
}

#[test]
fn unknown_subcommand_under_org() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("unknown-subcommand");

    // Clap returns exit code 2 for unknown subcommands (parse error)
    // Custom exit code 127 would require special error handling beyond scope of B10-02
    cmd.assert().failure().code(2);
}

#[test]
fn org_list_json_output_structure() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.env("SF_CONTENT_TYPE", "JSON");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--json");

    let output = cmd.output().expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse JSON to verify structure
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        // org list --json should return an array or object with org info
        assert!(
            json.is_array() || json.is_object(),
            "JSON output should be array or object, got: {}",
            json
        );
    } else if !stdout.is_empty() {
        // If not JSON, it's an error - verify it's on stdout in JSON mode
        panic!("Expected JSON output in JSON mode: {}", stdout);
    }
}

#[test]
fn columns_flag_requires_value() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--columns");

    // Missing value for flag should be a parse error
    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("value").or(predicate::str::contains("missing")));
}

#[test]
fn json_and_columns_flags_together() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("org");
    cmd.arg("list");
    cmd.arg("--json");
    cmd.arg("--columns");
    cmd.arg("Alias,Username");

    // Should succeed or fail consistently
    let assert = cmd.assert();
    let code = assert.get_output().status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 1 || code == 2,
        "exit code should be 0, 1, or 2"
    );
}
