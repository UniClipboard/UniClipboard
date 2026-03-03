# Milestones

## v0.1.0 — Download Progress

**Shipped:** 2026-03-03
**Phases:** 1 | **Plans:** 1

### Delivered

Added real-time download progress bar to update dialogs via Tauri IPC Channel.

### Accomplishments

- Bridged `download_and_install` callbacks to React via `tauri::ipc::Channel`
- Progress bar shows percentage when content-length known, indeterminate pulse otherwise
- Buttons disabled during download/install to prevent interruption
- i18n support (EN + ZH-CN)

### Git

- Commit: `8304405` feat(updater): add download progress display to update dialogs (#195)
- Tag: `v0.1.0`
