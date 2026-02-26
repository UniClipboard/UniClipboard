## 2026-02-25

- Added `ClipboardIntegrationMode` as a pure policy enum in `uc-app` usecase layer (`Full`/`Passive`) with boolean gate methods for observe/read/write.
- `clipboard/mod.rs` must export both module and type (`pub mod integration_mode;` + `pub use integration_mode::ClipboardIntegrationMode;`) to make the type available to callers.
- Verified from `src-tauri/` with `cargo check -p uc-app` to respect workspace command location rule.
- `RestoreClipboardSelectionUseCase` now needs mode injection to keep policy decisions in usecase layer; passive restore should fail fast before `set_next_origin` or OS writes.
- Constructor signature changes in `uc-app` require same-diff runtime accessor wiring updates in `uc-tauri/src/bootstrap/runtime.rs` to keep factory alignment.
- Passive restore regression test should assert both explicit error text and zero `write_snapshot` calls to guarantee no hidden OS clipboard side effects.
