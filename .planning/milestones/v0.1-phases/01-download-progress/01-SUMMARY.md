# Summary: Download Progress Display

**One-liner:** Added real-time download progress bar to update dialogs via Tauri IPC Channel.

**Shipped:** 2026-03-02
**Git commit:** 8304405 (feat(updater): add download progress display to update dialogs #195)

## What Was Built

Bridged Tauri updater's empty download callbacks to the React frontend using `tauri::ipc::Channel`. The progress bar shows download percentage when content-length is known, or an indeterminate pulse animation otherwise. Buttons are disabled during download/install to prevent interruption.

## Changes Made

| File                                                | Change                                                                          |
| --------------------------------------------------- | ------------------------------------------------------------------------------- |
| `src-tauri/crates/uc-tauri/src/commands/updater.rs` | Added `DownloadEvent` enum + `Channel<DownloadEvent>` param to `install_update` |
| `src/api/updater.ts`                                | Created Channel, accumulates chunk progress, fires `onProgress` callback        |
| `src/contexts/update-context.ts`                    | Added `DownloadProgress` type + `downloadProgress` context field                |
| `src/contexts/UpdateContext.tsx`                    | Manages progress state, wires Channel events to React state                     |
| `src/components/setting/AboutSection.tsx`           | Added progress bar + disabled buttons during download                           |
| `src/components/layout/Sidebar.tsx`                 | Same progress bar treatment as AboutSection                                     |
| `src/i18n/locales/en-US.json`                       | Added `downloading` / `installing` keys                                         |
| `src/i18n/locales/zh-CN.json`                       | Added Chinese translations                                                      |

## Key Decisions

- Used `let _ = on_event.send(...)` to silently ignore send errors (frontend may close dialog mid-download)
- `total === null` → indeterminate `animate-pulse` progress bar
- Derived `isInstalling` from `downloadProgress.phase !== 'idle'` — removed redundant local state

## Verification

- Rust: `cargo check` passes
- Frontend: `bun run build` passes
- Manual: Progress bar visible during update download/install flow
