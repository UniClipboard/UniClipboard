# Phase 26: Implement Global Sync Master Toggle and Improve Sync UX - Research

**Researched:** 2026-03-12
**Domain:** Sync policy enforcement (Rust engine) + React UI cascade disable + i18n
**Confidence:** HIGH

## Summary

This phase transforms the existing global `auto_sync` toggle from a simple default value into a true master switch that overrides all per-device sync settings. The implementation spans two layers: (1) the Rust sync engine where `apply_sync_policy()` must short-circuit before per-device evaluation when global auto_sync is false, and (2) the React frontend where the Devices page needs a warning banner and cascade-disabled controls.

The codebase is well-structured for this change. The `apply_sync_policy()` method in `sync_outbound.rs` already loads global settings at the top of the function -- adding an early return when `auto_sync` is false requires minimal code. On the frontend, `DeviceSettingsPanel` already implements a disable cascade pattern for per-device `auto_sync=off`, which can be extended to accept a `globalAutoSyncOff` prop. The i18n infrastructure (react-i18next with `en-US.json` and `zh-CN.json`) is ready for new keys.

**Primary recommendation:** Implement as two waves: (1) Backend engine enforcement + frontend banner/cascade disable, (2) i18n keys + description copy update.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Engine-layer enforcement: `apply_sync_policy` checks global `auto_sync` first, before any per-device resolution. If global is false, return empty peer list immediately -- no sync to any device
- This is a hard override: even devices with per-device `auto_sync=true` will not sync when global is off
- Settings loaded at runtime per clipboard change (existing pattern), so toggling takes effect immediately without restart
- Per-device settings are NOT modified when global is toggled -- they persist in storage unchanged
- When global auto_sync is re-enabled, all per-device settings automatically resume their previous state
- Global toggle acts as "overlay/override", not "erase" -- turning off is like pausing, not resetting
- No confirmation dialog on re-enable, just immediate resume
- When global auto_sync is off, ALL interactive controls in DeviceSettingsPanel are disabled (per-device auto_sync toggle, content type toggles, "Restore defaults" button) -- full cascade disable
- Per-device auto_sync toggle: grayed out, not clickable, but preserves its on/off visual state
- Warning banner at top of PairedDevicesPanel (above device list) when global auto_sync is off
- Banner style: amber/yellow background with warning icon -- soft warning, not alarming
- Banner includes text explaining sync is paused + a "Go to Settings" link that navigates to Settings page Sync section
- Banner completely disappears when global auto_sync is on -- no success toast, no fade animation
- Banner only appears in Devices page; Settings page does not need additional indicators
- SyncSection other settings (sync_frequency, max_file_size_mb) remain editable when auto_sync is off
- Label stays as "Auto Sync"; new description: EN: "Control clipboard sync across all devices. When disabled, no content will be synced to any device." / ZH: "控制所有设备的剪贴板同步。关闭后停止向所有设备同步内容。"
- Use existing react-i18next infrastructure
- No structural changes to page layout

### Claude's Discretion

- Exact banner component implementation (new component vs inline)
- Navigation mechanism for "Go to Settings" link (React Router navigation vs tab switching)
- Exact i18n key naming convention (follow existing patterns)
- Whether to add a subtle visual indicator on the global toggle to hint it's a master switch
- Exact disabled styling approach for DeviceSettingsPanel controls

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope
</user_constraints>

## Architecture Patterns

### Integration Points Map

```
Backend (Rust):
  sync_outbound.rs::apply_sync_policy()
    Line 67: loads global settings via self.settings.load()
    Line 86-103: per-device loop with resolve_sync_settings()
    ADD: early return empty vec if gs.sync.auto_sync == false (before line 81)

Frontend (React):
  SettingContext.tsx → provides setting.sync.auto_sync
  PairedDevicesPanel.tsx → add banner above device list
  DeviceSettingsPanel.tsx → accept globalAutoSyncOff prop, cascade disable
  SyncSection.tsx → update description i18n key
  i18n locales (en-US.json, zh-CN.json) → new keys
```

### Pattern 1: Engine-Level Global Override

**What:** Add early-return guard in `apply_sync_policy()` before the per-device loop.

**When to use:** When global auto_sync is false.

**Example:**

```rust
// In apply_sync_policy(), after loading global_settings (line 67-76):
if let Some(ref gs) = global_settings {
    if !gs.sync.auto_sync {
        info!("Global auto_sync is disabled; skipping all outbound sync");
        return vec![];
    }
}
// ... existing per-device loop continues below
```

This is a 3-line addition. The existing `global_settings` loading is already in place. The key insight is that this check must happen BEFORE the per-device `for peer in peers` loop at line 82.

### Pattern 2: Frontend Global State Propagation to Devices Page

**What:** PairedDevicesPanel reads global auto_sync state from SettingContext and passes it down.

**How it works in existing code:**

- `SettingContext` already provides `setting.sync.auto_sync` via `useSetting()` hook
- `PairedDevicesPanel` currently does NOT use `useSetting()` -- it only uses Redux `devicesSlice`
- `DeviceSettingsPanel` currently does NOT use `useSetting()` either

**Recommended approach:**

- `PairedDevicesPanel` calls `useSetting()` to read `setting?.sync.auto_sync`
- Passes `globalAutoSyncOff={!setting?.sync.auto_sync}` as prop to `DeviceSettingsPanel`
- Conditionally renders banner based on this value

### Pattern 3: Cascade Disable in DeviceSettingsPanel

**What:** When `globalAutoSyncOff` prop is true, all interactive controls become disabled.

**Existing pattern to extend (line 146-147):**

```tsx
const isAutoSyncOff = !settings?.auto_sync
const isDisabled = isComingSoon || isAutoSyncOff || isLoading
```

**Extended pattern:**

```tsx
// New prop
interface DeviceSettingsPanelProps {
  deviceId: string
  deviceName: string
  globalAutoSyncOff?: boolean // NEW
}

// In component body:
const isGlobalOff = globalAutoSyncOff ?? false
const isAutoSyncOff = !settings?.auto_sync

// For per-device auto_sync toggle:
// disabled={isGlobalOff || isLoading}  (but preserves visual checked state)

// For content type toggles:
const isDisabled = isComingSoon || isAutoSyncOff || isGlobalOff || isLoading

// For restore defaults button:
// disabled={isGlobalOff || isLoading}
```

### Pattern 4: Navigation to Settings Sync Section

**What:** Banner "Go to Settings" link navigates to Settings page with sync category active.

**Current routing:** Settings page uses `useState(DEFAULT_CATEGORY)` for active category. No URL-based category switching exists.

**Recommended approach:** Use `useNavigate()` with state parameter:

```tsx
// In PairedDevicesPanel banner:
const navigate = useNavigate()
navigate('/settings', { state: { category: 'sync' } })

// In SettingsPage, read initial category from location state:
const location = useLocation()
const [activeCategory, setActiveCategory] = useState(
  (location.state as { category?: string })?.category || DEFAULT_CATEGORY
)
```

This is the cleanest approach -- no URL query params, uses existing React Router state passing.

### Pattern 5: Warning Banner Component

**What:** Amber warning banner above device list in PairedDevicesPanel.

**Visual language reference from CONTEXT:** "Should feel like Phase 25's all-content-types-disabled warning -- same visual language."

**Phase 25 warning pattern (from DeviceSettingsPanel, not currently visible but referenced):** Inline amber/yellow warning with icon.

**Recommended implementation:**

```tsx
// Inline in PairedDevicesPanel, before the device list div
{
  globalAutoSyncOff && (
    <div className="mx-4 mt-6 flex items-center gap-3 rounded-lg border border-amber-500/20 bg-amber-500/10 px-4 py-3">
      <AlertTriangle className="h-4 w-4 text-amber-500 shrink-0" />
      <p className="text-sm text-amber-700 dark:text-amber-400">
        {t('devices.syncPaused.message')}{' '}
        <button
          type="button"
          onClick={() => navigate('/settings', { state: { category: 'sync' } })}
          className="font-medium underline hover:no-underline"
        >
          {t('devices.syncPaused.goToSettings')}
        </button>
      </p>
    </div>
  )
}
```

### Anti-Patterns to Avoid

- **Modifying per-device settings when global toggle changes:** The global toggle is an overlay, not a reset. Never write to per-device settings storage.
- **Adding global auto_sync state to Redux devicesSlice:** Global settings already live in SettingContext. Don't duplicate state.
- **Using CSS-only disable without `disabled` attribute:** Must set actual `disabled` on inputs for accessibility.
- **Adding fade/transition animations to banner:** Context explicitly says "no fade animation" for banner disappearance.

## Don't Hand-Roll

| Problem                       | Don't Build                            | Use Instead                           | Why                                       |
| ----------------------------- | -------------------------------------- | ------------------------------------- | ----------------------------------------- |
| Settings access in components | Custom Redux state for global sync     | `useSetting()` from SettingContext    | Already provides `setting.sync.auto_sync` |
| i18n strings                  | Hardcoded strings with language checks | `useTranslation()` + `t()`            | Existing react-i18next setup              |
| Navigation                    | window.location or custom routing      | `useNavigate()` from react-router-dom | Existing pattern throughout codebase      |
| Warning icon                  | Custom SVG                             | `AlertTriangle` from lucide-react     | Already imported in codebase              |

## Common Pitfalls

### Pitfall 1: Settings Load Failure in Engine

**What goes wrong:** `self.settings.load()` returns `Err` and the global check is skipped.
**Why it happens:** The current code already handles this with `Option<settings>` -- if load fails, all peers proceed.
**How to avoid:** The early-return for global auto_sync must be inside `if let Some(ref gs) = global_settings`, preserving the existing fallback behavior. If settings can't be loaded, sync proceeds (safety fallback).

### Pitfall 2: Banner Renders Before Settings Load

**What goes wrong:** `setting` from `useSetting()` is null during initial load, causing banner to flash briefly.
**How to avoid:** Guard with `setting?.sync.auto_sync !== false` -- only show banner when settings are loaded AND auto_sync is explicitly false. Default to "no banner" when settings are null.

### Pitfall 3: SettingsPage Ignores Navigation State

**What goes wrong:** User clicks "Go to Settings" from banner but lands on General tab (default).
**How to avoid:** Read `location.state.category` in SettingsPage `useState` initializer. Must also clear state after reading to prevent stale navigation on subsequent visits.

### Pitfall 4: Disabled Visual State vs Actual State

**What goes wrong:** Toggle appears "off" when it's actually "on" but globally disabled.
**How to avoid:** Keep `checked={settings?.auto_sync ?? true}` unchanged. Only add `disabled` attribute and reduced opacity styling. The visual on/off state must reflect the persisted per-device value.

## Code Examples

### Backend: Global auto_sync check in apply_sync_policy

```rust
// In sync_outbound.rs, apply_sync_policy method, after global_settings load:
if let Some(ref gs) = global_settings {
    if !gs.sync.auto_sync {
        info!("Global auto_sync disabled; returning empty peer list");
        return vec![];
    }
}
```

### Frontend: PairedDevicesPanel with banner

```tsx
// At top of PairedDevicesPanel component:
const { setting } = useSetting()
const navigate = useNavigate()
const globalAutoSyncOff = setting?.sync.auto_sync === false

// In render, before device list:
{
  globalAutoSyncOff && (
    <div className="mx-4 mt-6 ...amber styling...">
      <AlertTriangle className="h-4 w-4 text-amber-500 shrink-0" />
      <p className="text-sm ...">
        {t('devices.syncPaused.message')}{' '}
        <button onClick={() => navigate('/settings', { state: { category: 'sync' } })}>
          {t('devices.syncPaused.goToSettings')}
        </button>
      </p>
    </div>
  )
}
```

### Frontend: DeviceSettingsPanel cascade disable

```tsx
// Accept new prop:
interface DeviceSettingsPanelProps {
  deviceId: string
  deviceName: string
  globalAutoSyncOff?: boolean
}

// In toggle disable logic:
const isGlobalOff = globalAutoSyncOff ?? false

// Per-device auto_sync toggle: disabled={isGlobalOff || isLoading}
// Content type toggles: isDisabled = isComingSoon || isAutoSyncOff || isGlobalOff || isLoading
// Restore defaults: disabled={isGlobalOff || isLoading}
```

### i18n Keys to Add

```json
// en-US.json additions under "devices":
"syncPaused": {
  "message": "Clipboard sync is paused. All devices are currently not syncing.",
  "goToSettings": "Go to Settings"
}

// en-US.json update existing key:
"settings.sections.sync.autoSync.description":
  "Control clipboard sync across all devices. When disabled, no content will be synced to any device."

// zh-CN.json equivalents:
"syncPaused": {
  "message": "剪贴板同步已暂停，所有设备当前均不会同步内容。",
  "goToSettings": "前往设置"
}
"settings.sections.sync.autoSync.description":
  "控制所有设备的剪贴板同步。关闭后停止向所有设备同步内容。"
```

### SettingsPage: Read navigation state for category

```tsx
const location = useLocation()
const [activeCategory, setActiveCategory] = useState(
  (location.state as { category?: string })?.category || DEFAULT_CATEGORY
)
```

## Validation Architecture

### Test Framework

| Property                     | Value                                                         |
| ---------------------------- | ------------------------------------------------------------- |
| Framework (Backend)          | Rust built-in test + tokio::test (async)                      |
| Framework (Frontend)         | Vitest 4.x + @testing-library/jest-dom                        |
| Config file (Backend)        | Cargo.toml per crate (existing)                               |
| Config file (Frontend)       | None yet -- Wave 0 gap                                        |
| Quick run command (Backend)  | `cd src-tauri && cargo test -p uc-app --lib -- sync_outbound` |
| Quick run command (Frontend) | `bun run test -- --run`                                       |
| Full suite command           | `cd src-tauri && cargo test --workspace`                      |

### Phase Requirements to Test Map

| Req ID | Behavior                                                                | Test Type   | Automated Command                                                          | File Exists? |
| ------ | ----------------------------------------------------------------------- | ----------- | -------------------------------------------------------------------------- | ------------ |
| P26-01 | Global auto_sync=false returns empty peer list from apply_sync_policy   | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_global_toggle`      | No -- Wave 0 |
| P26-02 | Global auto_sync=false overrides per-device auto_sync=true              | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_global_override`    | No -- Wave 0 |
| P26-03 | Global auto_sync=true preserves existing per-device filtering behavior  | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_global_enabled`     | No -- Wave 0 |
| P26-04 | Settings load failure falls back to allowing all peers (safety)         | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_settings_fallback`  | No -- Wave 0 |
| P26-05 | Per-device settings NOT modified when global toggle changes             | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_no_device_mutation` | No -- Wave 0 |
| P26-06 | Banner visible when globalAutoSyncOff=true in PairedDevicesPanel        | manual-only | Visual inspection in `bun tauri dev`                                       | N/A          |
| P26-07 | Banner hidden when globalAutoSyncOff=false                              | manual-only | Visual inspection                                                          | N/A          |
| P26-08 | All DeviceSettingsPanel controls disabled when global off               | manual-only | Visual inspection                                                          | N/A          |
| P26-09 | Per-device toggle preserves checked visual state when globally disabled | manual-only | Visual inspection                                                          | N/A          |
| P26-10 | "Go to Settings" link navigates to sync category                        | manual-only | Visual inspection                                                          | N/A          |
| P26-11 | i18n keys render correctly in EN and ZH locales                         | manual-only | Visual inspection                                                          | N/A          |
| P26-12 | Re-enabling global auto_sync resumes per-device settings immediately    | integration | `cd src-tauri && cargo test -p uc-app -- sync_outbound_resume`             | No -- Wave 0 |

### Test Strategy by Layer

**Backend (Rust) -- Unit Tests for apply_sync_policy:**

The core enforcement logic lives in `SyncOutboundClipboardUseCase::apply_sync_policy()`. This method is `async` and takes `&self`, peers, and snapshot as arguments. Testing requires mock implementations of `SettingsPort` and `PairedDeviceRepositoryPort`.

**Existing test infrastructure:**

- `src-tauri/crates/uc-app/src/testing.rs` provides `NoopPairedDeviceRepository` and other noop impls
- `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs` has full mock implementations for all ports (InMemoryClipboard, StaticDeviceIdentity, etc.)
- These mocks can be reused or extended for targeted apply_sync_policy tests

**Key test scenarios for backend:**

1. **Global off, multiple peers with per-device on:** Create a `MockSettingsPort` returning `auto_sync=false`, provide 3 peers all with per-device `auto_sync=true`. Assert `apply_sync_policy()` returns empty vec.

2. **Global on, mixed per-device:** Create `MockSettingsPort` returning `auto_sync=true`, provide peers with mixed per-device settings. Assert only enabled peers returned (existing behavior preserved).

3. **Settings load error:** Create `MockSettingsPort` that returns `Err`. Assert all peers are returned (safety fallback).

4. **Global toggle cycle (off then on):** Two sequential calls -- first with `auto_sync=false` (empty result), then with `auto_sync=true` (peers returned based on per-device). Validates resume behavior.

**Frontend -- Manual Validation:**

Frontend tests are marked manual-only because:

- No vitest config exists yet (CLAUDE.md confirms "No test framework currently configured")
- The UI changes are primarily visual (banner styling, disabled states)
- Component testing would require mocking SettingContext + Redux store + React Router
- The effort of setting up frontend test infrastructure exceeds the scope of this phase

**Manual test checklist:**

1. Toggle global auto_sync OFF in Settings -> navigate to Devices -> verify amber banner visible
2. Verify all device controls are grayed out but preserve their on/off visual state
3. Click "Go to Settings" link -> verify lands on Sync section
4. Toggle global auto_sync ON -> navigate back to Devices -> verify banner gone, controls re-enabled
5. Verify other Sync section settings (sync_frequency, max_file_size_mb) remain editable when global off
6. Switch language to ZH -> verify all new strings render correctly

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-app -- sync_outbound -x`
- **Per wave merge:** `cd src-tauri && cargo test --workspace`
- **Phase gate:** Full backend suite green + manual frontend checklist before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/tests/sync_outbound_policy_test.rs` -- unit tests for global auto_sync enforcement in apply_sync_policy (covers P26-01 through P26-05, P26-12)
- [ ] Mock `SettingsPort` with configurable `auto_sync` value -- extend existing test mocks in `clipboard_sync_e2e_test.rs` or create new focused mock
- [ ] Frontend vitest config -- NOT required for this phase (manual testing sufficient for UI-only changes)

_(Note: The `apply_sync_policy` method is currently private (`async fn`, not `pub`). To unit test it directly, either make it `pub(crate)` or test through the `execute()` public method using the existing e2e test pattern. Recommendation: test through `execute()` with mock ports, matching the established pattern in `clipboard_sync_e2e_test.rs`.)_

## Sources

### Primary (HIGH confidence)

- Direct code inspection of `sync_outbound.rs` -- `apply_sync_policy()` at lines 60-121
- Direct code inspection of `paired_device.rs` -- `resolve_sync_settings()` at lines 29-34
- Direct code inspection of `DeviceSettingsPanel.tsx` -- existing disable pattern at lines 146-148
- Direct code inspection of `PairedDevicesPanel.tsx` -- component structure and imports
- Direct code inspection of `SyncSection.tsx` -- current auto_sync toggle and i18n usage
- Direct code inspection of `SettingContext.tsx` -- `useSetting()` providing `setting.sync.auto_sync`
- Direct code inspection of `devicesSlice.ts` -- Redux state structure
- Direct code inspection of `SettingsPage.tsx` -- category state management
- Direct code inspection of i18n locale files (en-US.json, zh-CN.json) -- existing key structure
- Direct code inspection of `uc-app/tests/clipboard_sync_e2e_test.rs` -- existing mock patterns
- Direct code inspection of `uc-app/src/testing.rs` -- shared noop implementations

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH -- all libraries already in project, no new dependencies
- Architecture: HIGH -- direct code inspection of all integration points
- Pitfalls: HIGH -- derived from understanding existing code patterns and edge cases
- Validation: HIGH -- existing test patterns in clipboard_sync_e2e_test.rs provide clear template

**Research date:** 2026-03-12
**Valid until:** 2026-04-12 (stable, no external dependency changes)
