# Oracle-derived fixtures

Captured from the pinned upstream CLI (`@salesforce/cli@2.158.4`, installed from
npm into an untracked directory) with empty state: no orgs and no auth files, so
nothing touches the OS keychain. Only the fields compared by `oracle_fixtures.rs`
are kept; stack traces and paths are dropped.

| File | Upstream command | Exit |
|---|---|---|
| org_list_empty.json | `org list --json` | 0 |
| org_logout_all_empty.json | `org logout --all --no-prompt --json` | 0 |
| org_display_no_default.json | `org display --json` | 1 |
| org_logout_no_target.json | `org logout --no-prompt --json` | 1 |
| org_display_unknown.json | `org display -o nope --json` | 2 |

Non-empty org shapes need the upstream credential store (the OS keychain) and are
not captured here; they belong to the opt-in live suite.
