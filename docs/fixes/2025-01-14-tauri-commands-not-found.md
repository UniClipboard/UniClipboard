**Date**: 2025-01-14
**Issue**: Frontend errors "Command not found" on application startup
**Status**: ✅ Resolved

## Problem Description

On application startup, the frontend encountered two critical errors:

1. `Command get_setup_state not found`
2. `Command enable_modern_window_style not found`

These errors prevented the application from starting properly.

## Root Cause Analysis

### Issue 1: `get_setup_state` Not Registered

**Root Cause**: The setup status command was defined in the legacy codebase but was not registered in the new architecture's `apps/gui/src-tauri/src/main.rs`.

**Evidence**:

- Frontend call: `invoke('get_setup_state')`
- Legacy definition: legacy API module
- Missing registration: `apps/gui/src-tauri/src/main.rs` (no entry for this command)

### Issue 2: `enable_modern_window_style` Not Registered

**Root Cause**: The `@cloudworxx/tauri-plugin-mac-rounded-corners` npm package was installed, but the Rust plugin code was not properly integrated into the project.

**Evidence**:

- Frontend import: `apps/gui/src/components/TitleBar.tsx:1` → `enableModernWindowStyle()`
- npm package: `package.json:26` → `@cloudworxx/tauri-plugin-mac-rounded-corners`
- Missing: No `src-tauri/src/plugins/` directory
- Missing: No command registration in `main.rs`

## Solution Implementation

### 1. Created Setup Status Command Module

**File**: `src-tauri/src/setup_status.rs` (new)

```rust
//! Simplified Setup Status Command (Temporary Implementation)
//! This is a minimal implementation to bridge the gap during architecture migration.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetupStatus {
    pub has_completed: bool,
    pub vault_initialized: bool,
    pub device_registered: bool,
    pub passphrase_set: bool,
}

#[tauri::command]
pub async fn get_setup_state() -> Result<SetupStatus, String> {
    // TODO: Integrate with new architecture's use cases
    Ok(SetupStatus {
        has_completed: false,
        vault_initialized: false,
        device_registered: false,
        passphrase_set: false,
    })
}
```

**Note**: This is a simplified temporary implementation that returns default values. The full implementation will be migrated to the hexagonal architecture.

### 2. Created macOS Rounded Corners Plugin Module

**File**: `src-tauri/src/plugins/mod.rs` (new)

```rust
pub mod mac_rounded_corners;
```

**File**: `src-tauri/src/plugins/mac_rounded_corners.rs` (new)

Copied from `node_modules/@cloudworxx/tauri-plugin-mac-rounded-corners/mod.rs`

Key commands:

- `enable_rounded_corners()` - Basic rounded corners
- `enable_modern_window_style()` - Rounded corners with shadow
- `reposition_traffic_lights()` - Traffic Lights positioning

### 3. Updated main.rs

**File**: `apps/gui/src-tauri/src/main.rs`

Added imports:

```rust
// Plugins
mod plugins;
use plugins::mac_rounded_corners;

// Setup status module (simplified implementation during migration)
mod setup_status;
use setup_status::get_setup_state;
```

Updated `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    // macOS Rounded Corners plugin commands
    mac_rounded_corners::enable_rounded_corners,
    mac_rounded_corners::enable_modern_window_style,
    mac_rounded_corners::reposition_traffic_lights,
    // Setup status command (simplified implementation during migration)
    get_setup_state,
])
```

## File Structure Changes

```
src-tauri/src/
├── main.rs              (modified - added imports and command registration)
├── setup_status.rs      (new - simplified status command implementation)
├── lib.rs               (unchanged - empty)
└── plugins/             (new directory)
    ├── mod.rs           (new - module exports)
    └── mac_rounded_corners.rs (new - macOS window styling)
```

## Verification

**Compilation**: ✅ Success

```
cargo check
Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.58s
```

**Application Start**: ✅ No command-not-found errors

## Known Issues / Warnings

The `mac_rounded_corners.rs` file generates warnings about `unexpected cfg condition value: cargo-clippy`. These are benign warnings from the `objc` crate macros and can be safely ignored or suppressed by updating the `objc` dependency:

```bash
cargo update -p objc
```

## Next Steps

### 1. Complete Setup Status Migration (High Priority)

The current `get_setup_state` implementation is a stub that returns hardcoded values. It needs to be properly integrated with the new architecture:

**Tasks**:

- [ ] Create `GetSetupState` use case in `uc-app/src/usecases/`
- [ ] Add accessor method to `UseCases` in `uc-tauri/src/bootstrap/runtime.rs`
- [ ] Update `setup_status.rs` command to use `runtime.usecases().get_setup_state()`
- [ ] Implement actual status checking logic (vault init, device registration, etc.)

**Reference**: Follow the pattern of `InitializeEncryption` command

### 2. Update Commands Status Documentation

**File**: `docs/architecture/commands-status.md`

Add the new commands to the tracking table:

- `get_setup_state` - mark as "Legacy bridge, needs migration"
- `enable_rounded_corners` - mark as "Plugin, external dependency"
- `enable_modern_window_style` - mark as "Plugin, external dependency"
- `reposition_traffic_lights` - mark as "Plugin, external dependency"

### 3. Consider Plugin Alternatives

The macOS rounded corners plugin is an external npm package. Consider:

- [ ] Evaluate if similar functionality can be implemented directly in the platform layer
- [ ] Review long-term maintenance implications of npm dependency
- [ ] Assess App Store compatibility (plugin claims compliance)

### 4. Testing

**Missing tests**:

- [ ] Add unit tests for `SetupStatus` serialization
- [ ] Add integration tests for command registration
- [ ] Test rounded corners plugin on actual macOS hardware

## Related Documentation

- [Commands Status Tracking](../architecture/commands-status.md)
- [Hexagonal Architecture Migration](../architecture/hexagonal-architecture.md) (if exists)
- [Tauri Commands Documentation](https://tauri.app/v2/api/javascript/namespace-commands/)
