//! Executable tests for `org list` and `org logout` against temporary native state.
//!
//! Each test seeds the org index file in a temp state dir and runs the real
//! binary in separate processes with the in-memory credential backend, so the
//! developer's `~/.sf`, `~/.sfdx`, and OS keychain are never touched.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use assert_cmd::Command;
use sf_core::auth::{OrgIndex, OrgIndexEntry};
use tempfile::TempDir;

fn seed(state: &TempDir) {
    let mut index = OrgIndex::new();
    index.add_org(
        OrgIndexEntry::new(
            "00Dprod",
            "prod",
            "a@example.com",
            "https://a.my.salesforce.com",
        )
        .set_default(true),
    );
    index.add_org(OrgIndexEntry::new(
        "00Ddev",
        "dev",
        "b@example.com",
        "https://b.my.salesforce.com",
    ));
    index
        .save_to_file(&state.path().join("org-index.json"))
        .expect("seed index");
}

fn sf(state: &TempDir, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("sf-rust")
        .expect("binary exists")
        .env("SF_RUST_STATE_DIR", state.path())
        .env("SF_RUST_CREDENTIAL_BACKEND", "memory")
        .env_remove("SF_CONTENT_TYPE")
        .args(args)
        .output()
        .expect("command runs")
}

fn json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("single JSON document on stdout")
}

#[test]
fn list_shows_seeded_orgs_sorted_and_marks_default_without_secrets() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let output = sf(&state, &["org", "list", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let orgs = json(&output)["result"]["nonScratchOrgs"].clone();
    assert_eq!(orgs[0]["alias"], "dev");
    assert_eq!(orgs[1]["alias"], "prod");
    assert_eq!(orgs[1]["isDefaultUsername"], true);
    assert_eq!(orgs[0]["isDefaultUsername"], false);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.to_lowercase().contains("token"));
}

#[test]
fn logout_requires_no_prompt_and_changes_nothing_without_it() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let output = sf(&state, &["org", "logout", "--all", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["code"], "PROMPT_UNSUPPORTED");
    let list = sf(&state, &["org", "list", "--json"]);
    assert_eq!(
        json(&list)["result"]["nonScratchOrgs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn logout_target_persists_across_processes_and_clears_default() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let output = sf(
        &state,
        &["org", "logout", "-o", "prod", "--no-prompt", "--json"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["result"][0], "a@example.com");

    let list = json(&sf(&state, &["org", "list", "--json"]));
    let orgs = list["result"]["nonScratchOrgs"].as_array().unwrap();
    assert_eq!(orgs.len(), 1);
    assert_eq!(orgs[0]["alias"], "dev");

    // The removed org's default status does not move to another org.
    let again = sf(&state, &["org", "logout", "--no-prompt", "--json"]);
    assert_eq!(again.status.code(), Some(1));
    assert_eq!(json(&again)["code"], "NoOrgSpecifiedWithNoPromptError");
}

#[test]
fn logout_unknown_target_fails_and_all_with_nothing_succeeds_empty() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let missing = sf(&state, &["org", "logout", "-o", "nope", "-p", "--json"]);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(json(&missing)["code"], "NoAuthFoundForTargetOrgError");

    let all = sf(&state, &["org", "logout", "--all", "-p", "--json"]);
    assert_eq!(all.status.code(), Some(0));
    assert_eq!(json(&all)["result"].as_array().unwrap().len(), 2);

    let empty = sf(&state, &["org", "logout", "--all", "-p", "--json"]);
    assert_eq!(empty.status.code(), Some(0));
    assert_eq!(json(&empty)["result"].as_array().unwrap().len(), 0);
}

#[test]
fn target_org_conflicts_with_all() {
    let state = TempDir::new().unwrap();
    let output = sf(&state, &["org", "logout", "-o", "x", "--all", "-p"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn corrupt_index_is_a_storage_error_not_an_empty_list() {
    let state = TempDir::new().unwrap();
    std::fs::write(state.path().join("org-index.json"), "{not json").unwrap();
    let output = sf(&state, &["org", "list", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["code"], "STORAGE_ERROR");
}

#[test]
fn display_without_target_or_default_fails_explicitly() {
    let state = TempDir::new().unwrap();
    let output = sf(&state, &["org", "display", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["code"], "NoDefaultEnvError");
}

#[test]
fn display_without_target_resolves_the_default_org_in_a_later_process() {
    let state = TempDir::new().unwrap();
    seed(&state);
    // "prod" is the seeded default; resolution reaches its (absent) credential.
    let output = sf(&state, &["org", "display", "--json"]);
    assert_eq!(json(&output)["code"], "CREDENTIAL_MISSING");
    assert!(String::from_utf8_lossy(&output.stdout).contains("prod"));
}

#[test]
fn display_unknown_org_and_invalid_api_version_fail_explicitly() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let missing = sf(&state, &["org", "display", "-o", "nope", "--json"]);
    // Upstream reports an unknown --target-org as a parse failure (exit 2).
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(json(&missing)["code"], "NamedOrgNotFoundError");

    let bad = sf(
        &state,
        &[
            "org",
            "display",
            "-o",
            "prod",
            "--api-version",
            "v61",
            "--json",
        ],
    );
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn display_reports_a_missing_credential_instead_of_inventing_details() {
    let state = TempDir::new().unwrap();
    seed(&state);
    // The index lists the org, but a separate process with the in-memory backend
    // holds no credential for it.
    let output = sf(&state, &["org", "display", "-o", "a@example.com", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["code"], "CREDENTIAL_MISSING");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("accessToken"));
}

#[test]
fn display_matches_org_id_selectors_too() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let output = sf(&state, &["org", "display", "-o", "00Ddev", "--json"]);
    // Resolved by org ID (then stops at the missing credential, not at selection).
    assert_eq!(json(&output)["code"], "CREDENTIAL_MISSING");
}

fn sf_headless(state: &TempDir, token: &str, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("sf-rust")
        .expect("binary exists")
        .env("SF_RUST_STATE_DIR", state.path())
        .env("SF_RUST_CREDENTIAL_BACKEND", "memory")
        .env("SF_ACCESS_TOKEN", token)
        .env("SF_RUST_INSTANCE_URL", "https://a.my.salesforce.com")
        .env_remove("SF_CONTENT_TYPE")
        .args(args)
        .output()
        .expect("command runs")
}

#[test]
fn headless_token_for_a_different_org_is_refused_and_touches_no_state() {
    let state = TempDir::new().unwrap();
    let output = sf_headless(
        &state,
        "00D000000000001AAA!secret-token-value",
        &["org", "display", "-o", "00D000000000002AAA", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["code"], "ORG_MISMATCH");
    let text = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("secret-token-value") && !err.contains("secret-token-value"));
    assert!(!state.path().join("org-index.json").exists());
}

#[test]
fn persistence_commands_are_refused_with_an_ephemeral_token() {
    let state = TempDir::new().unwrap();
    seed(&state);
    let token = "00D000000000001AAA!secret-token-value";
    for args in [
        vec!["org", "logout", "--all", "--no-prompt", "--json"],
        vec!["org", "login", "web", "--client-id", "x", "--json"],
    ] {
        let output = sf_headless(&state, token, &args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert_eq!(json(&output)["code"], "EPHEMERAL_INCOMPATIBLE");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-token-value"));
    }
    // Nothing was removed or added.
    let list = json(&sf(&state, &["org", "list", "--json"]));
    assert_eq!(
        list["result"]["nonScratchOrgs"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn malformed_headless_token_is_rejected_without_echoing_it() {
    let state = TempDir::new().unwrap();
    let output = sf_headless(
        &state,
        "secret-without-org-prefix",
        &["org", "display", "-o", "x", "--json"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["code"], "EPHEMERAL_INPUT_INVALID");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-without-org-prefix"));
}
