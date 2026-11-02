# 1. CLI Contract and Runtime

**Goal:** define one reliable process contract that all native commands share.

**Depends on:** B00-01 through B00-04. Command-specific workflows depend on this
package's output and error contracts.

## B10-01: Define typed command outcomes and process exits

- [ ] Add shared internal types for command success, warnings, and structured
  failures. Keep upstream JSON specifics in an explicit compatibility renderer,
  not in every workflow.
- [ ] Define exit-code mapping for parse/validation failures, operational
  failures, unsupported commands, and interruption. Choose and record behavior
  against the pinned baseline; don't copy JavaScript stack traces.
- [ ] Ensure errors preserve safe context and cannot expose credentials or full
  sensitive responses.

**Likely files**

- `crates/sf-cli/src/command.rs`
- `crates/sf-cli/src/error.rs`
- `crates/sf-cli/src/output.rs`
- `crates/sf-cli/tests/output_contract.rs`

**Acceptance**

- Tests assert JSON success and error envelopes, process exit status, stdout,
  and stderr independently.
- `SF_CONTENT_TYPE=JSON` behavior is covered for commands that support it.
- Errors never produce success-shaped fallbacks or claim a remote operation
  failed when its outcome is unknown.

## B10-02: Implement command parsing and explicit command registration

- [ ] Define the root parser and command tree using the selected `clap` setup.
- [ ] Keep registration tied to implemented command handlers. Unsupported
  commands must return a clear nonzero error, not a stub or subprocess fallback.
- [ ] Add tested helpers for repeated flags, mutually exclusive arguments,
  dependent flags, durations, org selection, and project requirements only as
  required by inventoried commands.
- [ ] Keep static help usable offline and keep user input validation at the
  command interface.

**Likely files**

- `crates/sf-cli/src/cli.rs`
- `crates/sf-cli/src/commands/mod.rs`
- `crates/sf-cli/tests/argument_contract.rs`

**Acceptance**

- Each registered command has a compatibility inventory entry and executable
  tests for valid and invalid arguments.
- Unknown flags and missing required context have stable tested exits.
- `--help`/`--version` never initialize auth or transport.

## B10-03: Add terminal output, JSON, and tracing adapters

- [ ] Implement JSON rendering as a single stdout document when JSON mode is
  active; direct diagnostics/progress to stderr.
- [ ] Implement human-readable output separately from wire/API models.
- [ ] Add structured tracing with redaction at fields' ownership points.
- [ ] Handle TTY detection, color/NO_COLOR policy, broken pipes, and non-TTY
  prompts without hanging.

**Likely files**

- `crates/sf-cli/src/output.rs`
- `crates/sf-cli/src/terminal.rs`
- `crates/sf-cli/src/diagnostics.rs`
- `crates/sf-cli/tests/output_contract.rs`

**Acceptance**

- JSON output parses as exactly one document; no ANSI/progress/log text leaks to
  stdout.
- Human output is tested for useful semantics without over-promising exact
  wording.
- A synthetic secret does not appear in logs, errors, or diagnostics.

## B10-04: Add graceful process lifecycle

- [ ] Handle termination signals and cancellation without panicking or detaching
  unfinished tasks.
- [ ] Ensure command completion and output flush happen before exit; handle
  broken-pipe behavior intentionally.
- [ ] Keep cancellation separate from rollback claims for remote writes.

**Likely files**

- `crates/sf-cli/src/main.rs`
- `crates/sf-cli/src/runtime.rs`
- `crates/sf-cli/tests/process_lifecycle.rs`

**Acceptance**

- Signal tests verify process status and cleanup using subprocesses.
- No spawned task outlives the process contract unnoticed.
- Long-running operation IDs are preserved by the workflow layer when applicable.
