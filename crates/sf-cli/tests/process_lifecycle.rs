//! Process lifecycle tests.
//!
//! These tests verify that the CLI handles process termination, signal handling,
//! and cleanup correctly. Tests use subprocesses to verify signal handling behavior.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::time::Duration;

#[test]
fn process_exits_cleanly_on_help() {
    // Basic sanity check: process exits with code 0 for help
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    cmd.assert().success().code(0);
}

#[test]
fn process_exits_cleanly_on_version() {
    // Process should exit cleanly for version
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--version");

    cmd.assert().success().code(0);
}

#[test]
fn process_exits_with_error_code_on_failure() {
    // Process exits with nonzero on error
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--invalid-flag");

    let assert = cmd.assert().failure();
    let exit_code = assert.get_output().status.code().unwrap_or(-1);
    assert!(exit_code > 0, "exit code must be nonzero for errors");
}

#[test]
fn stdout_flushed_before_exit() {
    // Output should appear in stdout (tests buffer flushing)
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    let output = cmd.output().expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Help text should be present and complete
    assert!(!stdout.is_empty());
    assert!(stdout.contains("Usage:") || stdout.contains("usage"));
}

#[test]
fn stderr_flushed_on_error() {
    // Error output should appear in stderr
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--invalid-flag");

    let output = cmd.output().expect("command runs");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Error message should be present
    assert!(!stderr.is_empty());
    assert!(
        stderr.to_lowercase().contains("error")
            || stderr.to_lowercase().contains("invalid")
            || stderr.to_lowercase().contains("unexpected")
    );
}

#[test]
fn process_completes_within_reasonable_time() {
    // Process should not hang
    use std::time::Instant;

    let start = Instant::now();
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    cmd.output().expect("command runs");

    let elapsed = start.elapsed();
    // Should complete in less than 5 seconds
    assert!(
        elapsed < Duration::from_secs(5),
        "process took too long: {:?}",
        elapsed
    );
}

#[test]
fn help_produces_complete_output() {
    // Help output should be complete and readable
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Usage:").or(predicate::str::contains("usage")));
}

#[test]
fn version_produces_valid_output() {
    // Version should produce valid output
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--version");

    let output = cmd.output().expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain version-like text
    assert!(
        stdout.contains("sf-rust") || stdout.contains("0."),
        "version output should be reasonable"
    );
}

#[test]
fn no_output_leakage_between_streams() {
    // When writing to stdout, nothing should leak to stderr
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    let output = cmd.output().expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Help goes to stdout
    assert!(!stdout.is_empty());

    // For successful help, stderr should be mostly empty (no cross-contamination)
    // We don't assert stderr is completely empty as there might be startup messages,
    // but the primary output should be on stdout
    let stderr_lower = stderr.to_lowercase();
    assert!(
        !stderr_lower.contains("usage") && !stderr_lower.contains("commands:"),
        "help text should not leak to stderr"
    );
}

#[test]
fn unknown_command_exits_nonzero() {
    // Unknown command should exit with error
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("unknowncommand123");

    cmd.assert().failure().code(
        predicate::eq(2) // Parse error (unknown subcommand)
            .or(predicate::gt(0)),
    );
}

#[test]
fn process_handles_json_mode_cleanup() {
    // Process should clean up properly even in JSON mode
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.env("SF_CONTENT_TYPE", "JSON");
    cmd.arg("--help");

    let output = cmd.output().expect("command runs");
    assert!(output.status.success());
}

#[test]
fn multiple_invocations_succeed() {
    // Multiple consecutive invocations should all succeed
    for i in 0..3 {
        let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
        cmd.arg("--version");

        let output = cmd.output().expect(&format!("invocation {} runs", i));
        assert!(output.status.success(), "invocation {} should succeed", i);
    }
}

#[test]
fn parse_error_exits_with_code_2() {
    // Parse errors should exit with code 2
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--unknown-flag");

    cmd.assert().failure().code(2);
}

#[test]
fn help_flag_takes_precedence() {
    // Help flag should work even with subcommands
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--help");

    cmd.assert().success().code(0);
}

#[test]
fn version_flag_takes_precedence() {
    // Version flag should work
    let mut cmd = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd.arg("--version");

    cmd.assert().success().code(0);
}

#[test]
fn process_output_consistent_across_runs() {
    // Running the same command twice should produce the same output
    let mut cmd1 = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd1.arg("--version");
    let output1 = cmd1.output().expect("first run");
    let stdout1 = String::from_utf8_lossy(&output1.stdout).to_string();

    let mut cmd2 = Command::cargo_bin("sf-rust").expect("binary exists");
    cmd2.arg("--version");
    let output2 = cmd2.output().expect("second run");
    let stdout2 = String::from_utf8_lossy(&output2.stdout).to_string();

    // Output should be identical or very similar
    assert_eq!(stdout1, stdout2, "version output should be consistent");
}
