//! Compares `sf-rust` offline JSON output with fixtures captured from the pinned
//! upstream CLI (see `fixtures/oracle/README.md`). Exit status is checked
//! separately from the JSON fields.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn fixture(name: &str) -> (i32, Value) {
    let path = format!(
        "{}/tests/fixtures/oracle/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    (doc["exit"].as_i64().unwrap() as i32, doc["body"].clone())
}

fn run(args: &[&str]) -> (i32, Value) {
    let state = TempDir::new().unwrap();
    let output = Command::cargo_bin("sf-rust")
        .unwrap()
        .env("SF_RUST_STATE_DIR", state.path())
        .env("SF_RUST_CREDENTIAL_BACKEND", "memory")
        .env_remove("SF_CONTENT_TYPE")
        .env_remove("SF_ACCESS_TOKEN")
        .env_remove("SFDX_ACCESS_TOKEN")
        .args(args)
        .output()
        .unwrap();
    let body = serde_json::from_slice(&output.stdout).expect("single JSON document");
    (output.status.code().unwrap(), body)
}

fn assert_matches(name: &str, args: &[&str]) {
    let (want_exit, want) = fixture(name);
    let (exit, got) = run(args);
    assert_eq!(exit, want_exit, "{name}: exit status");
    // Compare every field the fixture records; native-only extras are allowed.
    for (key, value) in want.as_object().unwrap() {
        assert_eq!(&got[key], value, "{name}: field {key}");
    }
}

#[test]
fn org_list_empty_matches_upstream() {
    assert_matches("org_list_empty.json", &["org", "list", "--json"]);
}

#[test]
fn org_logout_all_empty_matches_upstream() {
    assert_matches(
        "org_logout_all_empty.json",
        &["org", "logout", "--all", "--no-prompt", "--json"],
    );
}

#[test]
fn org_display_without_default_matches_upstream() {
    assert_matches("org_display_no_default.json", &["org", "display", "--json"]);
}

#[test]
fn org_logout_without_target_matches_upstream() {
    assert_matches(
        "org_logout_no_target.json",
        &["org", "logout", "--no-prompt", "--json"],
    );
}

#[test]
fn org_display_unknown_target_matches_upstream() {
    assert_matches(
        "org_display_unknown.json",
        &["org", "display", "-o", "nope", "--json"],
    );
}
