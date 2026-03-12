# Phase 26: Implement Global Sync Master Toggle and Improve Sync UX - Context

**Gathered:** 2026-03-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Transform the global `auto_sync` toggle from a mere default value into a true master switch that overrides all per-device sync settings. When the master toggle is off, all outbound sync stops regardless of per-device configurations. Additionally, improve the sync toggle description copy and add i18n support for new strings. Per-device settings are preserved (not erased) and automatically restored when the master is re-enabled.

</domain>

<decisions>
## Implementation Decisions

### Master toggle engine behavior

- Engine-layer enforcement: `apply_sync_policy` checks global `auto_sync` first, before any per-device resolution. If global is false, return empty peer list immediately — no sync to any device
- This is a hard override: even devices with per-device `auto_sync=true` will not sync when global is off
- Settings loaded at runtime per clipboard change (existing pattern), so toggling takes effect immediately without restart
- Per-device settings are NOT modified when global is toggled — they persist in storage unchanged

### Resume behavior

- When global auto_sync is re-enabled, all per-device settings automatically resume their previous state
- Global toggle acts as "overlay/override", not "erase" — turning off is like pausing, not resetting
- No confirmation dialog on re-enable, just immediate resume

### Device-level UI cascade disable

- When global auto_sync is off, ALL interactive controls in DeviceSettingsPanel are disabled:
  - Per-device auto_sync toggle: grayed out, not clickable, but preserves its on/off visual state
  - Content type toggles: all disabled (same as when per-device auto_sync is off)
  - "Restore defaults" button: disabled
- This is a full cascade disable — consistent with Phase 25's pattern where per-device auto_sync=off disables content type toggles

### Devices page banner

- When global auto_sync is off, show a warning banner at top of PairedDevicesPanel (above the device list)
- Banner style: amber/yellow background with warning icon — soft warning, not alarming
- Banner includes text explaining sync is paused + a "Go to Settings" link that navigates to Settings page Sync section
- Banner completely disappears when global auto_sync is on — no success toast, no fade animation
- Banner only appears in Devices page; Settings page does not need additional indicators

### Settings page behavior

- SyncSection other settings (sync_frequency, max_file_size_mb) remain editable when auto_sync is off — allows pre-configuration
- Only auto_sync toggle affects sync engine behavior; other settings are pure configuration

### Toggle description copy

- Label stays as "Auto Sync" (consistent terminology with per-device level)
- New description: functional style emphasizing master switch role
- EN: "Control clipboard sync across all devices. When disabled, no content will be synced to any device."
- ZH: "控制所有设备的剪贴板同步。关闭后停止向所有设备同步内容。"

### i18n approach

- Use existing react-i18next infrastructure (already set up in the project)
- Add new translation keys for: banner text, banner link text, updated toggle description
- Both zh-CN and en locale files updated

### Layer hierarchy communication

- No structural changes to page layout — hierarchy communicated through banner + disabled states only
- Users understand the global→device relationship when they see the banner in Devices page
- No additional explanatory text needed in Settings page

### Claude's Discretion

- Exact banner component implementation (new component vs inline)
- Navigation mechanism for "Go to Settings" link (React Router navigation vs tab switching)
- Exact i18n key naming convention (follow existing patterns)
- Whether to add a subtle visual indicator on the global toggle to hint it's a master switch
- Exact disabled styling approach for DeviceSettingsPanel controls

</decisions>

<specifics>
## Specific Ideas

- The banner in Devices page should feel like Phase 25's all-content-types-disabled warning — same visual language
- Global auto_sync description should read professionally, not like a tooltip or help text

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `SyncSection.tsx`: Global sync settings component with auto_sync toggle — needs description update and i18n
- `DeviceSettingsPanel.tsx`: Already has `isAutoSyncOff` pattern for disabling content type toggles — extend to check global state
- `PairedDevicesPanel.tsx`: Device list component — add banner at top
- `SettingContext.tsx`: Provides global settings with `updateSyncSetting()` — already supports runtime updates
- `devicesSlice.ts`: Redux slice with `deviceSyncSettings` state — may need global sync state
- react-i18next setup with `useTranslation()` hook, locale files in `src/i18n/locales/`
- Phase 25 all-disabled warning pattern in DeviceSettingsPanel — reusable visual pattern for banner

### Established Patterns

- Per-device settings stored as JSON column with null = use global defaults (`resolve_sync_settings()`)
- Settings loaded from storage each time (not cached) — Phase 24 decision
- Redux thunks for async state management with immediate API calls
- `apply_sync_policy()` in sync_outbound.rs for peer filtering — integration point for global check
- `useTranslation()` hook + `t()` function for i18n strings

### Integration Points

- `SyncOutboundClipboardUseCase::apply_sync_policy()` — add global auto_sync check at top, before per-device loop
- `SyncSection.tsx` — update description text, add i18n keys
- `PairedDevicesPanel.tsx` — add conditional banner component
- `DeviceSettingsPanel.tsx` — accept global auto_sync state as prop, cascade disable all controls
- `SettingContext.tsx` or Redux — expose global auto_sync state to Devices page
- i18n locale files (zh-CN, en) — add new translation keys

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 26-implement-global-sync-master-toggle-and-improve-sync-ux_
_Context gathered: 2026-03-12_
