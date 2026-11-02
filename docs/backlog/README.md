# Implementation Backlog

This backlog turns the design specs into ordered work that can be implemented
and reviewed in small slices. It is the implementation source of truth until
the project adopts an issue tracker. Keep task IDs when tasks move to issues.

## Project constraints

- The shipped CLI is Rust-native. Node.js is permitted only to run the pinned
  upstream comparison oracle in isolated tests.
- No command is `supported` until its inventory entry and required tests pass.
- Do not add a placeholder command that exits successfully without doing the
  documented operation.
- Keep each crate's public interface small. Add a crate or adapter only for a
  demonstrated ownership, testability, or platform need.
- `sf-rust` is the development executable name. Do not overwrite or implicitly
  invoke the installed `sf`.
- Keep live Salesforce tests opt-in, isolated, least-privilege, and credential
  free in ordinary local and pull-request tests.

See [architecture](../technical-architecture.md),
[development requirements](../development-setup.md),
[testing strategy](../testing-strategy.md), and
[security requirements](../security-compliance.md). Those documents define the
normative behavior; this backlog defines work order and likely file ownership.

## Work order

| Order | Work package | Depends on | Outcome |
|---|---|---|---|
| 0 | [Workspace foundation](00-foundation.md) | None | A pinned, linted, tested Rust workspace and minimal executable. |
| 1 | [CLI contract and runtime](10-cli-runtime.md) | 0 | Stable parser, output, error, and process behavior for future commands. |
| 2 | [Configuration and authentication](20-state-auth.md) | 0, CLI contract from 1 | Isolated native state and testable org/auth workflows. |
| 3 | [Salesforce API transport](30-salesforce-api.md) | 0, auth/session interfaces from 2 | Safe, tested REST and SOAP transport primitives. |
| 4 | [Projects and metadata workflows](40-project-metadata.md) | 1, 2, 3 | Supported project loading and deploy/retrieve vertical slices. |
| 5 | [Parity and release](50-parity-release.md) | Each released workflow's package | First-party commands graduated through compatibility and release gates. |

Within a package, tasks are ordered unless marked independent. Tasks with unmet
dependencies stay unchecked. Begin at `B00-01`; the repository currently has
design documents only, so no task should be marked complete based on prose.

## Completion rule

Check a task only after its files exist, acceptance criteria pass, tests are
committed, and relevant documentation is updated. If implementation reveals a
contradiction in the design, amend the design and this backlog in the same
change. Record intentional compatibility differences rather than silently
changing the contract.
