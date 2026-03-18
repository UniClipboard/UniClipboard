---
status: awaiting_human_verify
trigger: 'CLI commands output a wall of tracing logs mixed with command output'
created: 2026-03-18T00:00:00Z
updated: 2026-03-18T00:00:00Z
---

## Current Focus

hypothesis: CLI uses same LogProfile as GUI/daemon, sending all tracing output to console
test: Implement Cli profile with console=off, json=info
expecting: CLI output will be clean with only command results
next_action: Implement 5-step plan from implementation_plan

## Symptoms

expected: CLI commands like `uniclipboard-cli space-status` should only output the command result
actual: Output is mixed with tracing logs (INFO, DEBUG), eprintln messages like "Sentry DSN not set", database/migration logs
errors: Not an error - log noise pollution in CLI output
reproduction: Run any CLI command
started: Since CLI was added in phase 41

## Eliminated

(none yet)

## Evidence

- timestamp: 2026-03-18T00:00:00Z
  checked: profile.rs, builders.rs, tracing.rs, main.rs, command files
  found: CLI uses same build_core() -> init_tracing_subscriber() path as GUI/daemon with no profile override. Console layer outputs to stdout with Dev profile (debug level). eprintln("Sentry DSN not set") in tracing.rs also adds noise.
  implication: Root cause confirmed - CLI needs its own LogProfile with console=off

## Resolution

root_cause: CLI uses the same tracing initialization as GUI/daemon, which outputs debug/info logs to console. Additionally eprintln statements in tracing.rs add noise.
fix: Added LogProfile::Cli variant (console=off, json=info), wired it through build_cli_context, added --verbose flag to CLI, removed noisy eprintln in tracing.rs
verification: cargo check passes for full workspace, all 53 uc-observability tests pass including 3 new Cli-specific tests
files_changed:

- src-tauri/crates/uc-observability/src/profile.rs
- src-tauri/crates/uc-observability/src/init.rs
- src-tauri/crates/uc-bootstrap/src/builders.rs
- src-tauri/crates/uc-bootstrap/src/lib.rs
- src-tauri/crates/uc-bootstrap/src/tracing.rs
- src-tauri/crates/uc-cli/src/main.rs
- src-tauri/crates/uc-cli/src/commands/space_status.rs
- src-tauri/crates/uc-cli/src/commands/devices.rs
- src-tauri/crates/uc-cli/src/commands/status.rs
- src-tauri/crates/uc-cli/Cargo.toml
