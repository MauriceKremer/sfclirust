//! Argument contract tests for command parsing and validation.
//!
//! These tests verify that commands parse arguments correctly, validate input,
//! and handle edge cases with proper exit codes and error messages.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn sf(state: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.env("SF_RUST_STATE_DIR", state.path());
    cmd.env("SF_RUST_CREDENTIAL_BACKEND", "memory");
    cmd.env_remove("SF_CONTENT_TYPE");
    cmd
}

#[test]
fn org_list_empty_state_succeeds() {
    let state = TempDir::new().expect("temp dir");
    sf(&state).args(["org", "list"]).assert().success();
}

#[test]
fn org_list_json_has_all_groups() {
    let state = TempDir::new().expect("temp dir");
    let output = sf(&state)
        .args(["org", "list", "--json"])
        .output()
        .expect("command runs");
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["status"], 0);
    assert!(value["warnings"].is_array());
    let value = value["result"].clone();
    for group in [
        "other",
        "sandboxes",
        "nonScratchOrgs",
        "devHubs",
        "scratchOrgs",
    ] {
        assert!(value[group].is_array(), "missing group {group}");
    }
}

#[test]
fn org_list_rejects_columns_flag_not_in_upstream_contract() {
    let state = TempDir::new().expect("temp dir");
    sf(&state)
        .args(["org", "list", "--columns", "Alias,Username"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn org_list_invalid_flag() {
    let state = TempDir::new().expect("temp dir");
    sf(&state)
        .args(["org", "list", "--invalid-flag"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unknown").or(predicate::str::contains("unexpected")));
}

#[test]
fn org_subcommand_help() {
    let state = TempDir::new().expect("temp dir");
    sf(&state)
        .args(["org", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("org").or(predicate::str::contains("Org")));
}

#[test]
fn org_list_help() {
    let state = TempDir::new().expect("temp dir");
    sf(&state)
        .args(["org", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list").or(predicate::str::contains("List")));
}

#[test]
fn unknown_subcommand_under_org() {
    let state = TempDir::new().expect("temp dir");
    // Clap returns exit code 2 for unknown subcommands (parse error)
    sf(&state)
        .args(["org", "unknown-subcommand"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn org_login_without_a_method_is_a_parse_error() {
    let state = TempDir::new().expect("temp dir");
    sf(&state).args(["org", "login"]).assert().failure().code(2);
}
