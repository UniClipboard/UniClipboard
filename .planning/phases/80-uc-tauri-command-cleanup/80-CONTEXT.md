# Phase 80: uc-tauri Command Cleanup - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning
**Source:** PRD Express Path (docs/plans/frontend-direct-daemon-connection.md)

<domain>
## Phase Boundary

Remove all Tauri commands that have been migrated to daemon HTTP API. Retain only Tauri-specific commands: daemon lifecycle, auth bootstrap, system tray, quick panel, preview panel, updater, autostart, and protocol handler. Target: reduce uc-tauri commands code by 60%+.

</domain>

<decisions>
## Implementation Decisions

### Commands to Remove

- `get_clipboard_entries` — replaced by GET /clipboard/entries
- `get_clipboard_entry` — replaced by GET /clipboard/entries/:id
- `delete_clipboard_entry` — replaced by DELETE /clipboard/entries/:id
- `restore_clipboard_entry` — already proxying to daemon (Phase 72)
- `toggle_favorite_clipboard_item` — replaced by POST /clipboard/entries/:id/favorite
- `get_clipboard_stats` — replaced by GET /clipboard/stats
- `get_settings` / `update_settings` — replaced by GET/PUT /settings
- `get_encryption_state` / `unlock_encryption_session` — replaced by /encryption/\* endpoints
- Storage-related commands — replaced by /storage/\* endpoints

### Commands to Retain

- `daemon` module: start/stop daemon, connection info, daemon_connect_info
- `auth` module: getToken(), verify() — bootstrap auth for frontend
- `lifecycle` module: app lifecycle management (partial)
- `tray` module: system tray operations
- `quick_panel` module: macOS quick access panel
- `preview_panel` module: content preview panel
- `updater` module: Tauri update mechanism
- `autostart` module: launch at login
- `protocol` module: dent:// protocol handler

### Cleanup Approach

- Remove command functions from respective files
- Remove from invoke_handler![] registration in main.rs
- Remove associated use case accessors if now unused
- Remove DTO types that are no longer needed in uc-tauri
- Clean up imports and dependencies

### Claude's Discretion

- Whether to delete entire command files or leave stubs
- Cleanup of associated test code
- Whether to keep debug/development-only commands
- Cargo dependency cleanup in uc-tauri/Cargo.toml

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Tauri Commands

- `src-tauri/crates/uc-tauri/src/commands/` — All command files
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — Primary removal target
- `src-tauri/crates/uc-tauri/src/commands/encryption.rs` — Removal target
- `src-tauri/crates/uc-tauri/src/commands/settings.rs` — Removal target
- `src-tauri/crates/uc-tauri/src/commands/storage.rs` — Removal target

### Command Registration

- `src-tauri/src/main.rs` — invoke_handler![] registration

### Frontend API

- `src/api/` — Verify no remaining Tauri invoke calls to removed commands

### Daemon API

- Phases 74-76 output — Daemon endpoints that replace commands

</canonical_refs>

<specifics>
## Specific Ideas

- clipboard.rs is the largest command file (~48K) — bulk of the reduction
- encryption.rs (~35K) is second largest
- After removal, uc-tauri should be a thin shell: daemon management + Tauri-native features
- Some commands may have been partially migrated in earlier phases (e.g., restore in Phase 72)
- Need to verify frontend has no remaining invoke() calls to removed commands

</specifics>

<deferred>
## Deferred Ideas

- Removing DaemonWsBridge and related Rust-side event forwarding (may keep for compatibility)
- Further uc-tauri crate splitting or simplification
- Complete elimination of uc-tauri model types

</deferred>

---

_Phase: 80-uc-tauri-command-cleanup_
_Context gathered: 2026-03-29 via PRD Express Path_
