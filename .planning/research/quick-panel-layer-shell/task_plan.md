# Quick Panel Wayland Layer Shell

## Goal
Retain the React/WebView UI and implement native Layer Shell hosting on supported Wayland desktops, including Omarchy activation and a verified focus/paste sequence.

## Phases
- [x] Research GTK/Tauri lifecycle and compositor APIs; lock BRIEF.md.
- [x] Implement Linux panel backend and compositor integration.
- [x] Add meaningful lifecycle/layout/input tests and integration guidance.
- [x] Build and validate the isolated lifecycle/focus/paste sequence; report remaining limits.

## Next Step
Implementation and automated validation complete. Interactive acceptance still needs physical outside-click delivery, IME and multiple outputs; the virtual pointer fixture did not deliver clicks even to an ordinary GTK control window. Runtime installation and personal compositor binding are documented, not applied.

## Constraints
- Preserve existing user edits in Cargo.toml/Cargo.lock (local Engine overrides).
- Keep platform integration in desktop-owned adapters; no Engine edits.
- No clipboard contents, filenames, paths or window titles in diagnostics or persistent research artifacts.
- Preserve native macOS/Windows backends and capability-based support for other Linux desktops.
