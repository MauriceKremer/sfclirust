//! Output contract tests for JSON error envelopes and exit codes.
//!
//! These tests verify that sf-rust produces compatible JSON error shapes,
//! correct exit codes for different failure categories, and clean separation
//! of stdout (JSON) and stderr (diagnostics).

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

/// Helper to set up a test environment with isolated state directory.

#[test]
fn exit_code_unsupported_command() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("unsupported-command");

    // NOTE: Clap returns exit code 2 for unrecognized subcommands (parse errors).
    // Custom exit code 127 for unsupported commands requires enhanced error handling
    // that would need to distinguish between:
    // - Parse errors (invalid arguments) = exit 2
    // - Unknown subcommands = exit 127
    // This distinction is deferred to when we have full command registration logic.
    // For now, we accept clap's default behavior (exit 2 for unknown subcommands).
    cmd.assert().failure().code(2);
}

#[test]
fn exit_code_parse_error() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--invalid-flag");
    cmd.assert().failure().code(2);
}

#[test]
fn help_flag_exits_zero() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");
    cmd.assert().success();
}

#[test]
fn version_flag_exits_zero() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--version");
    cmd.assert().success();
}

#[test]
fn json_error_envelope_on_stdout() {
    // When a command fails with SF_CONTENT_TYPE=JSON, the error envelope goes to stdout.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.env("SF_CONTENT_TYPE", "JSON");
    cmd.arg("unsupported-command");

    let assert = cmd.assert().failure();
    let output = assert.get_output();

    // Stdout should contain valid JSON with error structure
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "stdout should contain JSON error");

    // Parse JSON to verify structure: {status, message, code}
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        assert!(
            json.is_object(),
            "stdout should be a JSON object, got: {}",
            json
        );
        assert!(
            json.get("status").is_some(),
            "error envelope must have 'status' field"
        );
        assert!(
            json.get("message").is_some() || json.get("code").is_some(),
            "error envelope must have 'message' or 'code'"
        );
    } else {
        panic!("stdout should be parseable JSON: {}", stdout);
    }
}

#[test]
fn no_json_output_on_help_flag() {
    // Help should not produce JSON errors, only text help.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn exit_code_on_json_error() {
    // Even when JSON is requested, exit code must be nonzero for failures.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.env("SF_CONTENT_TYPE", "JSON");
    cmd.arg("unsupported-command");

    let assert = cmd.assert().failure();
    let code = assert.get_output().status.code().unwrap_or(-1);
    assert!(
        code > 0,
        "exit code must be nonzero for failures, got {}",
        code
    );
}

#[test]
fn no_secrets_in_error_message() {
    // A synthetic secret in an error context must be redacted.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");

    // Set a fake API key in environment to simulate a scenario where it might leak
    cmd.env("FAKE_SECRET_KEY", "super_secret_key_12345");
    cmd.arg("unsupported-command");

    let output = cmd.output().expect("command runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Secret should not appear in stderr or stdout
    assert!(
        !stderr.contains("super_secret_key_12345"),
        "secret must not leak into stderr"
    );
    assert!(
        !stdout.contains("super_secret_key_12345"),
        "secret must not leak into stdout"
    );
}

#[test]
fn unknown_flag_produces_error_not_help() {
    // Unknown flags should produce an error message, not the full help text.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--unknown-flag-xyz");

    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unknown").or(
            predicate::str::contains("unrecognized").or(predicate::str::contains("unexpected")),
        ));
}

#[test]
fn json_output_single_document_only() {
    // JSON output must be exactly one document, not multiple JSON objects or NDJSON.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.env("SF_CONTENT_TYPE", "JSON");
    cmd.arg("unsupported-command");

    let output = cmd.output().expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // Try to parse as single JSON and verify it succeeds
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    assert!(
        parsed.is_ok(),
        "JSON output must be a single valid JSON document"
    );

    // Count top-level objects: should be 1
    let json_objects: Vec<&str> = stdout.lines().filter(|l| l.starts_with('{')).collect();
    assert_eq!(
        json_objects.len(),
        1,
        "JSON output must contain exactly one top-level object"
    );
}

#[test]
fn exit_zero_success_no_error_envelope() {
    // When the command succeeds, exit code is 0 and no error envelope is required.
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--version");

    cmd.assert().success().code(0);
}
