/**
 * `src/api/tauri-command/` — TS wrappers for Tauri commands that go directly
 * to the in-process `AppFacade` (does NOT route through the daemon webserver).
 *
 * This is the GUI in-process facade entry point; future GUI features should
 * land here, and the existing `src/api/daemon/*` HTTP path will be migrated
 * over time. See the project memory `project_gui_uses_inprocess_facade.md`.
 *
 * Backend equivalents live in `src-tauri/crates/uc-tauri/src/commands/`.
 */

export * from './mobile_sync'
