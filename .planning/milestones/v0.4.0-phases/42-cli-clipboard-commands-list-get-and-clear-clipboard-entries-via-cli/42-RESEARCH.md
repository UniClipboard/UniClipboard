# Phase 42: CLI Clipboard Commands — Research

**Researched:** 2026-03-19
**Domain:** CLI command implementation for clipboard operations (list, get, clear)
**Confidence:** HIGH

## Summary

Phase 42 adds three clipboard subcommands to the existing `uniclipboard-cli` binary: `clipboard list`, `clipboard get <id>`, and `clipboard clear`. All three are "direct mode" commands — they bootstrap via `build_cli_context()` + `build_non_gui_runtime()` and query the SQLite database directly, without requiring the daemon to be running.

The existing CLI skeleton (Phase 41) provides a well-established pattern: clap-based argument parsing, `output::print_result()` for dual JSON/human output, exit code constants, and the bootstrap flow (`CliBootstrapContext` → `build_non_gui_runtime()` → `CoreUseCases`). All three required use cases already exist in `uc-app`: `ListClipboardEntryProjections::execute()` for listing with rich projections, `GetEntryDetailUseCase::execute()` for single entry detail with full content, and `ClearClipboardHistory::execute()` for bulk clear.

**Primary recommendation:** Follow the exact pattern from `commands/devices.rs` — bootstrap runtime, call existing use case, map result to a Serialize+Display DTO, print via `output::print_result()`. Use clap's nested subcommand (`Clipboard { List, Get, Clear }`) to group clipboard operations under a single `clipboard` parent command.

<phase_requirements>

## Phase Requirements

| ID         | Description                              | Research Support                                                                                                                                                                                                                           |
| ---------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| CLI-F01    | CLI clipboard history list/show commands | `CoreUseCases::list_entry_projections()` returns `EntryProjectionDto` with preview, content_type, size, timestamps. `CoreUseCases::get_entry_detail()` returns full content for a single entry. Both accessible via direct-mode bootstrap. |
| (implicit) | CLI clipboard clear command              | `CoreUseCases::clear_clipboard_history()` returns `ClearHistoryResult` with deleted_count and failed_entries.                                                                                                                              |

</phase_requirements>

## Standard Stack

### Core (already in workspace)

| Library            | Version    | Purpose                                                         | Why Standard           |
| ------------------ | ---------- | --------------------------------------------------------------- | ---------------------- |
| clap               | 4.5        | CLI argument parsing with derive                                | Already used in uc-cli |
| serde + serde_json | 1.x        | JSON serialization for --json output                            | Already used in uc-cli |
| tokio              | 1.x (full) | Async runtime for use case execution                            | Already used in uc-cli |
| anyhow             | 1.0        | Error handling                                                  | Already used in uc-cli |
| uc-bootstrap       | workspace  | Runtime construction (build_cli_context, build_non_gui_runtime) | Established pattern    |
| uc-app             | workspace  | Use cases (CoreUseCases)                                        | Established pattern    |
| uc-core            | workspace  | Domain types (EntryId)                                          | Established pattern    |
| uc-observability   | workspace  | LogProfile::Cli for verbose mode                                | Established pattern    |

### No New Dependencies Required

All needed libraries are already in `uc-cli/Cargo.toml`. No new crate additions needed.

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-cli/src/
├── main.rs              # Add Clipboard subcommand group with nested subcommands
├── commands/
│   ├── mod.rs           # Add clipboard module
│   ├── clipboard.rs     # NEW: list, get, clear subcommands
│   ├── devices.rs       # Existing (reference pattern)
│   ├── space_status.rs  # Existing
│   └── status.rs        # Existing (RPC pattern, not used here)
├── output.rs            # Existing print_result (reuse as-is)
└── exit_codes.rs        # Existing exit codes (reuse as-is)
```
