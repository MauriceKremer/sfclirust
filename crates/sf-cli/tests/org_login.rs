//! Executable tests for the browser login command.
//!
//! No browser is launched (`SF_RUST_NO_BROWSER`), no Salesforce host is
//! contacted, and state lives in a temp dir with the in-memory credential
//! backend. The tests drive the loopback callback the way a browser would, so
//! they cover the command wiring up to (not including) the token exchange,
//! which needs a real Salesforce origin and belongs to the opt-in live run.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use tempfile::TempDir;

fn base_command(state: &TempDir) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_sf-rust"));
    cmd.env("SF_RUST_STATE_DIR", state.path())
        .env("SF_RUST_CREDENTIAL_BACKEND", "memory")
        .env("SF_RUST_NO_BROWSER", "1")
        .env("SF_RUST_CALLBACK_PORT", "0")
        .env_remove("SF_RUST_CLIENT_ID")
        .env_remove("SF_CONTENT_TYPE")
        .args(["org", "login", "web", "--json"]);
    cmd
}

struct Login {
    child: Child,
    url: String,
}

/// Start a login with a client ID and read the printed authorization URL.
fn start_login(
    state: &TempDir,
    timeout_secs: &str,
) -> (Login, BufReader<std::process::ChildStderr>) {
    let mut child = base_command(state)
        .args(["--client-id", "test-client"])
        .env("SF_RUST_LOGIN_TIMEOUT_SECS", timeout_secs)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr"));
    let url = loop {
        let mut line = String::new();
        let read = stderr.read_line(&mut line).expect("read stderr");
        assert!(read > 0, "process exited before printing the URL");
        if line.starts_with("https://") {
            break line.trim().to_string();
        }
    };
    (Login { child, url }, stderr)
}

fn callback_port(url: &str) -> u16 {
    let rest = url.split("localhost%3A").nth(1).expect("redirect_uri port");
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("port")
}

fn query_param(url: &str, name: &str) -> String {
    let query = url.split('?').nth(1).expect("query");
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .expect("param")
        .to_string()
}

fn send_callback(port: u16, query: &str) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    write!(
        stream,
        "GET /oauth/callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
    )
    .expect("write");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
}

fn finish(login: Login) -> (Option<i32>, serde_json::Value) {
    let output = login.child.wait_with_output().expect("wait");
    let json = serde_json::from_slice(&output.stdout).expect("one JSON document on stdout");
    (output.status.code(), json)
}

#[test]
fn authorization_url_uses_pkce_loopback_and_no_browser_launch() {
    let state = TempDir::new().unwrap();
    let (login, _stderr) = start_login(&state, "1");
    assert!(
        login
            .url
            .starts_with("https://login.salesforce.com/services/oauth2/authorize?")
    );
    assert!(login.url.contains("code_challenge_method=S256"));
    assert!(login.url.contains("client_id=test-client"));
    let (code, json) = finish(login);
    assert_eq!(code, Some(1));
    assert_eq!(json["code"], "LOGIN_TIMEOUT");
}

#[test]
fn denied_authorization_fails_and_stores_nothing() {
    let state = TempDir::new().unwrap();
    let (login, _stderr) = start_login(&state, "30");
    let port = callback_port(&login.url);
    let oauth_state = query_param(&login.url, "state");
    send_callback(port, &format!("state={oauth_state}&error=access_denied"));
    let (code, json) = finish(login);
    assert_eq!(code, Some(1));
    assert_eq!(json["code"], "LOGIN_DENIED");
    assert!(!state.path().join("org-index.json").exists());
}

#[test]
fn callback_with_wrong_state_is_rejected() {
    let state = TempDir::new().unwrap();
    let (login, _stderr) = start_login(&state, "30");
    let port = callback_port(&login.url);
    send_callback(port, "state=forged&code=abc");
    let (code, json) = finish(login);
    assert_eq!(code, Some(1));
    assert_eq!(json["code"], "INVALID_CALLBACK");
}

#[cfg(unix)]
#[test]
fn interrupt_stops_the_login_and_reports_it() {
    let state = TempDir::new().unwrap();
    let (login, _stderr) = start_login(&state, "60");
    let status = Command::new("kill")
        .args(["-INT", &login.child.id().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    let (code, json) = finish(login);
    assert_eq!(code, Some(130));
    assert_eq!(json["code"], "INTERRUPTED");
}

#[test]
fn missing_client_id_fails_before_any_listener_or_state() {
    let state = TempDir::new().unwrap();
    let output = base_command(&state).output().expect("run");
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["code"], "MISSING_CLIENT_ID");
}

#[test]
fn non_salesforce_or_non_https_instance_url_is_a_validation_error() {
    let state = TempDir::new().unwrap();
    for url in ["http://login.salesforce.com", "https://evil.example.com"] {
        let output = base_command(&state)
            .args(["--client-id", "x", "--instance-url", url])
            .output()
            .expect("run");
        assert_eq!(output.status.code(), Some(2), "{url}");
    }
}

#[test]
fn deferred_login_flags_remain_unregistered() {
    let state = TempDir::new().unwrap();
    for flag in ["--browser", "--set-default-dev-hub", "--scopes"] {
        let output = base_command(&state)
            .args([flag, "x"])
            .output()
            .expect("run");
        assert_eq!(output.status.code(), Some(2), "{flag}");
    }
}

#[cfg(unix)]
#[test]
fn terminate_signal_exits_with_143_and_no_stored_state() {
    let state = TempDir::new().unwrap();
    let (login, _stderr) = start_login(&state, "60");
    let status = Command::new("kill")
        .args(["-TERM", &login.child.id().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
    let (code, json) = finish(login);
    assert_eq!(code, Some(143));
    assert_eq!(json["code"], "INTERRUPTED");
    assert!(!state.path().join("org-index.json").exists());
}
