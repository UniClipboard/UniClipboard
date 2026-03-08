---
status: resolved
trigger: 'Investigate issue: autostart-settings-chain'
created: 2026-03-08T00:00:00Z
updated: 2026-03-08T12:50:00Z
---

## Current Focus

hypothesis: OS-level autostart is currently controlled only from the frontend via @tauri-apps/plugin-autostart, while the backend settings pipeline (UpdateSettings use case) never calls AutostartPort; additionally, the ApplyAutostartSetting use case contains a logic bug and is unused. Wiring ApplyAutostartSetting into the settings::update_settings Tauri command and simplifying the frontend toggle to rely on this backend behavior will establish the desired chain: Settings.general.auto_start → ApplyAutostartSetting → AutostartPort (TauriAutostart) → tauri-plugin-autostart.
test: Implement three changes: (1) fix ApplyAutostartSetting to call enable/disable based on the requested enabled flag, not the old setting; (2) extend uc-tauri settings::update_settings to, when general.auto_start changes, construct a TauriAutostart adapter and invoke ApplyAutostartSetting before persisting settings; (3) simplify GeneralSection.handleAutoStartChange to only call updateGeneralSetting and update local state, removing direct plugin enable/disable calls. Then run cargo check in src-tauri and, if feasible, a basic manual test path or unit tests for settings update behavior.
expecting: After changes, toggling the "launch at login" switch triggers the backend settings.update_settings command, which detects the auto_start change and calls ApplyAutostartSetting, which in turn uses TauriAutostart (AutostartPort) to invoke tauri-plugin-autostart. The frontend no longer calls the plugin directly for enable/disable, reducing duplication. Cargo check passes and the code compiles.
next_action: Resolved. Human verified via logs that toggling auto_start in Settings UI triggers the full chain through ApplyAutostartSetting to OS-level autostart.

## Symptoms

expected: Toggling the auto_start option in Settings should enable/disable OS-level autostart for UniClipboard on login (at least on macOS first, eventually multi-platform), by flowing through a use case that calls AutostartPort which in turn calls tauri-plugin-autostart.
actual: The Settings auto_start option can be changed in the UI, but there is currently no implemented chain that detects this field change and invokes AutostartPort/tauri-plugin-autostart, so OS-level autostart is not actually updated. The user has not yet verified post-reboot behavior; investigation should first confirm whether any autostart wiring exists and, if not, design and implement the missing chain.
errors: No known error messages yet; user has not inspected logs specifically for autostart-related errors. Start from code inspection.
reproduction: From the Settings screen in the desktop app, toggle the auto_start/"launch at login" switch, apply/save if needed, then (once implemented) reboot or log out/in to verify whether UniClipboard starts automatically. Currently, expectation is that this has no effect because the backend chain is missing.
timeline: This behavior is believed to be "never implemented" rather than a regression; the user notes that the chain "is simply not implemented at all".

## Eliminated

- hypothesis: No autostart infrastructure exists in the codebase (no port, adapter, or use case)
  evidence: Found uc-platform::ports::AutostartPort, uc-platform::usecases::ApplyAutostartSetting, uc-tauri::adapters::TauriAutostart that wraps tauri-plugin-autostart, and uc-tauri::commands::autostart Tauri commands; main.rs registers tauri_plugin_autostart and exposes autostart commands.
  timestamp: 2026-03-08T00:08:00Z

## Evidence

- timestamp: 2026-03-08T00:05:00Z
  checked: uc-platform/src/ports/autostart.rs
  found: AutostartPort trait with is_enabled/enable/disable methods using anyhow::Result.
  implication: Port abstraction for OS-level autostart exists at the platform layer.

- timestamp: 2026-03-08T00:06:00Z
  checked: uc-platform/src/usecases/apply_autostart.rs
  found: ApplyAutostartSetting use case loads Settings via SettingsPort, compares general.auto_start to the requested enabled flag, but currently calls autostart.enable/disable based on the OLD settings.general.auto_start value instead of the requested enabled flag, then updates settings.general.auto_start and saves.
  implication: There is a dedicated use case intended to synchronize Settings.general.auto_start with OS-level autostart via AutostartPort, but it contains a logic bug and is not wired into commands.

- timestamp: 2026-03-08T00:06:30Z
  checked: uc-tauri/src/adapters/autostart.rs
  found: TauriAutostart implements AutostartPort using tauri_plugin_autostart::ManagerExt on AppHandle (autolaunch().is_enabled/enable/disable).
  implication: Concrete AutostartPort adapter is implemented using tauri-plugin-autostart and is ready to be wired.

- timestamp: 2026-03-08T00:07:00Z
  checked: uc-tauri/src/commands/autostart.rs and src-tauri/src/main.rs registration
  found: enable_autostart/disable_autostart/is_autostart_enabled Tauri commands directly call app_handle.autolaunch() without going through AutostartPort or ApplyAutostartSetting; main.rs registers tauri_plugin_autostart and these commands.
  implication: There are two parallel autostart integration paths: direct Tauri commands using the plugin, and the port/use case path that is not currently used by commands.

- timestamp: 2026-03-08T00:07:30Z
  checked: src/components/setting/GeneralSection.tsx
  found: Frontend imports @tauri-apps/plugin-autostart, checks isEnabled() on mount to set local autoStart state, and on toggle calls updateGeneralSetting({ auto_start: checked }) followed by direct enable()/disable() calls; on OS error it rolls back the backend setting.
  implication: Frontend currently manages autostart by calling the plugin directly, treating backend Settings.general.auto_start as a secondary persisted flag, not as the driver of OS state through a use case.

- timestamp: 2026-03-08T00:08:30Z
  checked: uc-app/src/usecases/update_settings.rs and uc-tauri/src/commands/settings.rs
  found: update_settings command parses JSON into Settings, compares old vs new to detect changes (including general.auto_start), logs diffs, and persists settings via UpdateSettings use case; there is no call to ApplyAutostartSetting or AutostartPort in this path.
  implication: Backend settings update path does not apply OS-level autostart changes; only the frontend plugin call currently affects autostart.

- timestamp: 2026-03-08T01:00:00Z
  checked: All four modified files after applying fix
  found: cargo check passes with 0 errors. Frontend no longer imports @tauri-apps/plugin-autostart. The chain is now Settings UI toggle -> updateGeneralSetting -> update_settings command -> ApplyAutostartSetting -> TauriAutostart -> tauri-plugin-autostart.
  implication: The fix compiles correctly and the autostart chain is wired end-to-end.

## Resolution

root_cause: The autostart infrastructure (AutostartPort, ApplyAutostartSetting use case, TauriAutostart adapter, and autostart Tauri commands) exists but is not integrated into the settings update flow. The React GeneralSection toggles OS autostart by calling the @tauri-apps/plugin-autostart API directly, while the uc-app UpdateSettings use case only persists Settings.general.auto_start without invoking ApplyAutostartSetting. Additionally, ApplyAutostartSetting currently computes enable/disable based on the previous settings.general.auto_start value instead of the requested enabled flag. As a result, there is no single, use-case-driven chain from Settings.auto_start to AutostartPort/tauri-plugin-autostart, and OS-level autostart is controlled solely by the frontend instead of the backend settings pipeline.
fix: |
Four changes applied:

1. Simplified ApplyAutostartSetting use case (uc-platform) to only handle OS-level autostart
   via AutostartPort — removed SettingsPort dependency and settings load/save logic.
   Fixed the logic bug: now uses the `enabled` parameter (not old settings value) to decide
   enable vs disable. Added is_enabled() check to skip redundant calls.
2. Added `apply_autostart()` accessor to UseCases in runtime.rs that constructs TauriAutostart
   from AppHandle and returns ApplyAutostartSetting instance.
3. Extended update_settings command to detect auto_start changes, call apply_autostart use case,
   and rollback settings if OS-level autostart fails.
4. Simplified frontend GeneralSection: removed direct @tauri-apps/plugin-autostart imports and
   calls; autoStart state now reads from setting.general.auto_start; handleAutoStartChange
   only calls updateGeneralSetting (backend handles OS autostart).
   verification: cargo check passes (0 errors, 2 unrelated warnings). Manual verification confirmed via logs — toggling auto_start from false→true triggers the full chain: update_settings → ApplyAutostartSetting → "OS autostart enabled".
   files_changed:

- src-tauri/crates/uc-platform/src/usecases/apply_autostart.rs
- src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
- src-tauri/crates/uc-tauri/src/commands/settings.rs
- src/components/setting/GeneralSection.tsx
