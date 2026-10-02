# uc-tauri

Tauri adapter layer for UniClipboard.

## Purpose

This crate contains Tauri-specific implementations that were previously in `uc-platform`.
The separation enforces architecture boundaries:

- **uc-platform** is now free of Tauri dependencies
- **uc-tauri** contains all Tauri-specific code (adapters, bootstrap, commands)
- **pub(crate) constructors** prevent Tauri types from leaking to other crates

## Architecture

```text
crates/uc-tauri/   # Tauri-specific adapters (this crate): commands, tray, quick panel, run loop
src-tauri/         # Tauri packaging shell: the `uniclipboard` bin, tauri.conf.json, icons, capabilities
```

## Modules

- `adapters/` - Tauri implementations of ports (TauriAutostart, TauriUiPort)
- `bootstrap/` - Two-phase initialization (AppRuntimeSeed, run_app)
- `commands/` - Tauri command handlers and DTOs (future)

## Usage

```rust
// Phase 1: Create runtime seed (before Tauri setup)
let seed = uc_tauri::bootstrap::create_runtime()?;

// Phase 2: Build runtime inside Tauri setup
tauri::Builder::default()
    .setup(move |app| {
        let runtime = uc_tauri::bootstrap::build_runtime(seed, app.handle())?;
        app.manage(runtime);
        Ok(())
    })
    .run(tauri::generate_context!())?;
```

## Architecture Guardrails

- All Tauri-specific types (AppHandle, etc.) stay in uc-tauri
- Adapters use `pub(crate)` constructors
- Other crates depend only on uc-core ports
