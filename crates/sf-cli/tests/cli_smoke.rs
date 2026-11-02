#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_flag_works() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("failed to build binary");
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("sf-rust"))
        .stdout(predicate::str::contains("help"));
}

#[test]
fn version_flag_works() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("failed to build binary");
    cmd.arg("--version");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("0.1.0"));
}

#[test]
fn no_args_shows_help() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("failed to build binary");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("sf-rust"));
}

#[test]
fn unknown_flag_fails() {
    let mut cmd = Command::cargo_bin("sf-rust").expect("failed to build binary");
    cmd.arg("--unknown-flag");

    cmd.assert().failure();
}
