//! Org-related commands (list, logout; display and login are not yet registered).
//!
//! State lives in the native global state directory (`SF_RUST_STATE_DIR`
//! overrides it). Credentials use the OS secure store unless
//! `SF_RUST_CREDENTIAL_BACKEND=memory` selects the in-memory test backend.

use crate::cli::{OrgCommand, OrgListArgs, OrgLoginSubcommands, OrgLogoutArgs, OrgSubcommands};

use crate::error::{CommandError, CommandOutcome};
use serde_json::{Value, json};
use sf_core::auth::{EphemeralCredential, OrgIndexEntry, PersistentOrgStore};
use sf_core::config::paths::global_config_path;
use std::path::PathBuf;

const INDEX_FILE: &str = "org-index.json";
const KEYRING_SERVICE: &str = "sf-rust";
const BACKEND_ENV: &str = "SF_RUST_CREDENTIAL_BACKEND";

/// Execute an org subcommand.
pub async fn execute(cmd: OrgCommand) -> CommandOutcome {
    match cmd.subcommand {
        OrgSubcommands::Login(login) => match login.subcommand {
            OrgLoginSubcommands::Web(args) => {
                if let Some(error) = reject_ephemeral("org login web") {
                    return CommandOutcome::Failure(error);
                }
                super::org_login::web(args).await
            }
        },
        OrgSubcommands::Display(args) => super::org_display::display(args).await,
        OrgSubcommands::List(args) => list(args),
        OrgSubcommands::Logout(args) => {
            if let Some(error) = reject_ephemeral("org logout") {
                return CommandOutcome::Failure(error);
            }
            logout(args)
        }
    }
}

/// Commands that write or delete persisted credentials cannot run alongside an
/// ephemeral token: it is never stored, so there is nothing to persist or remove.
fn reject_ephemeral(command: &str) -> Option<CommandError> {
    match EphemeralCredential::from_lookup(|name| std::env::var(name).ok()) {
        Ok(None) => None,
        Ok(Some(_)) => Some(
            CommandError::operational(
                "EPHEMERAL_INCOMPATIBLE",
                format!("'{command}' changes persisted credentials and cannot run with an ephemeral access token"),
            )
            .with_context(
                "Unset SF_ACCESS_TOKEN/SFDX_ACCESS_TOKEN to use persisted logins".to_string(),
            ),
        ),
        Err(error) => Some(CommandError::operational(
            "EPHEMERAL_INPUT_INVALID",
            error.to_string(),
        )),
    }
}

pub(super) fn storage_error(message: impl std::fmt::Display) -> CommandError {
    CommandError::operational("STORAGE_ERROR", format!("Org storage error: {message}"))
}

pub(super) fn open_store() -> Result<(PersistentOrgStore, PathBuf), Box<CommandError>> {
    let index_path = global_config_path(INDEX_FILE).map_err(|e| Box::new(storage_error(e)))?;
    let use_memory = std::env::var(BACKEND_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("memory"));
    let store = if use_memory {
        PersistentOrgStore::with_memory_backend(&index_path)
    } else {
        PersistentOrgStore::with_os_backend(&index_path, KEYRING_SERVICE)
    }
    .map_err(|e| Box::new(storage_error(e)))?;
    Ok((store, index_path))
}

fn sorted_entries(store: &PersistentOrgStore) -> Vec<OrgIndexEntry> {
    let mut entries: Vec<OrgIndexEntry> = store.list_orgs().into_iter().cloned().collect();
    entries.sort_by(|a, b| {
        (a.alias.to_lowercase(), &a.org_id).cmp(&(b.alias.to_lowercase(), &b.org_id))
    });
    entries
}

fn org_json(entry: &OrgIndexEntry) -> Value {
    json!({
        "alias": entry.alias,
        "username": entry.username,
        "orgId": entry.org_id,
        "instanceUrl": entry.instance_url,
        "isDefaultUsername": entry.is_default,
    })
}

/// List authenticated orgs from the native index (no keychain access, no network).
///
/// The index records no org type, so every org is reported under
/// `nonScratchOrgs`; the other upstream groups are always present and empty.
fn list(_args: OrgListArgs) -> CommandOutcome {
    let (store, _) = match open_store() {
        Ok(opened) => opened,
        Err(error) => return CommandOutcome::Failure(*error),
    };
    let orgs: Vec<Value> = sorted_entries(&store).iter().map(org_json).collect();
    CommandOutcome::Success {
        result: Some(json!({
            "other": [],
            "sandboxes": [],
            "nonScratchOrgs": orgs,
            "devHubs": [],
            "scratchOrgs": [],
        })),
        warnings: vec![],
    }
}

/// Resolve an alias, username, or org ID to exactly one stored org.
///
/// Aliases win over usernames, which win over org IDs; an ambiguous match at the
/// first level that matches anything is an error and never falls through.
pub(super) fn resolve_target(
    entries: &[OrgIndexEntry],
    target: &str,
    not_found: fn(&str) -> CommandError,
) -> Result<OrgIndexEntry, Box<CommandError>> {
    let levels: [&dyn Fn(&OrgIndexEntry) -> bool; 3] = [
        &|e| e.alias.eq_ignore_ascii_case(target),
        &|e| e.username.eq_ignore_ascii_case(target),
        &|e| e.org_id.eq_ignore_ascii_case(target),
    ];
    for level in levels {
        let matches: Vec<&OrgIndexEntry> = entries.iter().filter(|e| level(e)).collect();
        match matches.as_slice() {
            [] => {}
            [one] => return Ok((*one).clone()),
            _ => {
                return Err(Box::new(CommandError::operational(
                    "AMBIGUOUS_ORG",
                    format!("Org selector '{target}' matches multiple authenticated orgs"),
                )));
            }
        }
    }
    Err(Box::new(not_found(target)))
}

/// Upstream `org logout` error for an unknown target (exit 1).
fn logout_not_found(target: &str) -> CommandError {
    CommandError::operational(
        "NoAuthFoundForTargetOrgError",
        format!("No authenticated org found with the {target} username or alias."),
    )
}

/// Select the entries a logout applies to without ever prompting.
fn select_logout_targets(
    entries: &[OrgIndexEntry],
    args: &OrgLogoutArgs,
) -> Result<Vec<OrgIndexEntry>, Box<CommandError>> {
    if args.all {
        return Ok(entries.to_vec());
    }
    if let Some(target) = &args.target_org {
        return resolve_target(entries, target, logout_not_found).map(|entry| vec![entry]);
    }
    match entries.iter().find(|e| e.is_default) {
        Some(default) => Ok(vec![default.clone()]),
        None => Err(Box::new(CommandError::operational(
            "NoOrgSpecifiedWithNoPromptError",
            "You must specify a target-org (or default target-org config is set) or use --all flag when using the --no-prompt flag.",
        ))),
    }
}

/// Remove stored credentials and index entries locally. No remote revocation is
/// attempted; the access and refresh tokens remain valid until they expire.
fn logout(args: OrgLogoutArgs) -> CommandOutcome {
    if !args.no_prompt {
        return CommandOutcome::Failure(
            CommandError::operational(
                "PROMPT_UNSUPPORTED",
                "Confirmation prompts are not supported; pass --no-prompt to log out",
            )
            .with_context("Logout removes local credentials only".to_string()),
        );
    }
    let (mut store, index_path) = match open_store() {
        Ok(opened) => opened,
        Err(error) => return CommandOutcome::Failure(*error),
    };
    let entries = sorted_entries(&store);
    let targets = match select_logout_targets(&entries, &args) {
        Ok(targets) => targets,
        // With --all and nothing stored, upstream succeeds with an empty result.
        Err(error) => return CommandOutcome::Failure(*error),
    };
    let mut removed = Vec::new();
    for target in &targets {
        match store.remove_org(&target.org_id, &target.alias, &index_path) {
            Ok(_) => removed.push(target.username.clone()),
            Err(error) => {
                return CommandOutcome::Failure(storage_error(error).with_context(format!(
                    "Removed {} org(s) before the failure; rerun logout to finish",
                    removed.len()
                )));
            }
        }
    }
    let warnings = if removed.is_empty() {
        vec![]
    } else {
        vec!["Remote token revocation was not attempted".to_string()]
    };
    CommandOutcome::Success {
        result: Some(json!(removed)),
        warnings,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    fn entry(alias: &str, username: &str, default: bool) -> OrgIndexEntry {
        OrgIndexEntry::new(
            format!("00D{alias}"),
            alias,
            username,
            "https://example.my.salesforce.com",
        )
        .set_default(default)
    }

    fn args(target: Option<&str>, all: bool) -> OrgLogoutArgs {
        OrgLogoutArgs {
            target_org: target.map(str::to_string),
            all,
            no_prompt: true,
        }
    }

    #[test]
    fn target_matches_alias_or_username_case_insensitively() {
        let entries = vec![
            entry("prod", "a@x.com", false),
            entry("dev", "b@x.com", true),
        ];
        let by_alias = select_logout_targets(&entries, &args(Some("PROD"), false)).unwrap();
        assert_eq!(by_alias[0].alias, "prod");
        let by_user = select_logout_targets(&entries, &args(Some("b@x.com"), false)).unwrap();
        assert_eq!(by_user[0].alias, "dev");
    }

    #[test]
    fn unknown_or_ambiguous_target_is_an_error() {
        let entries = vec![
            entry("a", "same@x.com", false),
            entry("b", "same@x.com", false),
        ];
        let missing = select_logout_targets(&entries, &args(Some("nope"), false)).unwrap_err();
        assert_eq!(missing.code, "NoAuthFoundForTargetOrgError");
        let ambiguous =
            select_logout_targets(&entries, &args(Some("same@x.com"), false)).unwrap_err();
        assert_eq!(ambiguous.code, "AMBIGUOUS_ORG");
    }

    #[test]
    fn default_is_used_and_missing_default_never_prompts() {
        let with_default = vec![entry("a", "a@x.com", true)];
        let picked = select_logout_targets(&with_default, &args(None, false)).unwrap();
        assert_eq!(picked[0].alias, "a");
        let none = vec![entry("a", "a@x.com", false)];
        let error = select_logout_targets(&none, &args(None, false)).unwrap_err();
        assert_eq!(error.code, "NoOrgSpecifiedWithNoPromptError");
    }

    #[test]
    fn all_selects_everything_including_nothing() {
        assert!(
            select_logout_targets(&[], &args(None, true))
                .unwrap()
                .is_empty()
        );
        let entries = vec![entry("a", "a@x.com", false), entry("b", "b@x.com", false)];
        assert_eq!(
            select_logout_targets(&entries, &args(None, true))
                .unwrap()
                .len(),
            2
        );
    }
}
