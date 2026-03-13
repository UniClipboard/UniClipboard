# Phase 27: Settings Page - Keyboard Shortcuts Settings - Research

**Researched:** 2026-03-13
**Domain:** React Settings UI + Keyboard Shortcut Customization
**Confidence:** HIGH

## Summary

This phase adds a "Shortcuts" settings category to the existing Settings page. The codebase already has a robust shortcut infrastructure: definitions with scope/layer priority, conflict detection (`getCandidateKeyIssues`), key normalization, and override resolution (`resolveShortcuts` with `ShortcutKeyOverrides`). The `react-hotkeys-hook@5.2.1` library already installed includes `useRecordHotkeys` for key capture.

The main work involves: (1) uncommenting ~8 reserved shortcut definitions, (2) creating a `ShortcutsSection` component with key recording UI, (3) adding a `keyboard_shortcuts` field to both Rust `Settings` and TypeScript `Settings` types, (4) wiring overrides through `SettingContext` to `ShortcutProvider`/`useShortcut`.

**Primary recommendation:** Follow the exact patterns of existing settings sections (SettingGroup/SettingRow), use `useRecordHotkeys` from react-hotkeys-hook for key capture, store overrides as `Record<string, string | string[]>` via existing settings persistence.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- New independent category "Shortcuts" in Settings sidebar, positioned after "Appearance" and before "Sync"
- Uses lucide `Command` icon
- Title: "Shortcuts"
- Expose ALL shortcuts -- activate the ~8 commented-out shortcut definitions in `definitions.ts`
- Group by scope (global, clipboard, settings, devices, modal) with section headers
- Click-to-record key binding capture: user clicks edit button, then presses desired key combination
- Esc cancels recording mode
- Real-time conflict detection using existing `getCandidateKeyIssues` during recording
- When conflict detected: show conflict info inline, user can choose to confirm (override) or cancel
- Store custom key overrides in Rust settings system (new `keyboard_shortcuts` field)
- Read/write via existing `get_settings` / `update_settings` Tauri commands
- Per-shortcut reset button (rotate-ccw icon) next to each shortcut to restore individual default
- "Reset All Shortcuts" button at bottom of section to restore all defaults
- Override format matches existing `ShortcutKeyOverrides` type: `Record<string, string | string[]>`
- Reset button only appears when a shortcut has been modified from its default

### Claude's Discretion

- Exact layout and spacing of the shortcut list rows
- Animation/transition for recording state
- How to display modifier key symbols (command/ctrl/alt/shift vs text)
- Whether to show scope descriptions or just scope names as section headers
- Error state and edge case handling

### Deferred Ideas (OUT OF SCOPE)

None
</user_constraints>

## Standard Stack

### Core

| Library            | Version     | Purpose                                                      | Why Standard                                  |
| ------------------ | ----------- | ------------------------------------------------------------ | --------------------------------------------- |
| react-hotkeys-hook | 5.2.1       | `useRecordHotkeys` for key capture, `useHotkeys` for binding | Already installed, has built-in recording API |
| lucide-react       | (installed) | `Command` icon for sidebar                                   | Already used for all settings category icons  |
| react-i18next      | (installed) | i18n for all UI text                                         | Established pattern in every settings section |

### Supporting

| Library                 | Version | Purpose                                     | When to Use                             |
| ----------------------- | ------- | ------------------------------------------- | --------------------------------------- |
| @/shortcuts/conflicts   | local   | `getCandidateKeyIssues`, `resolveShortcuts` | Conflict detection during key recording |
| @/shortcuts/normalize   | local   | `normalizeHotkey`                           | Normalize recorded keys for comparison  |
| @/shortcuts/definitions | local   | `SHORTCUT_DEFINITIONS`, `ShortcutScope`     | Source of truth for all shortcuts       |

### Alternatives Considered

| Instead of         | Could Use                  | Tradeoff                                                                       |
| ------------------ | -------------------------- | ------------------------------------------------------------------------------ |
| `useRecordHotkeys` | Custom `keydown` listener  | Unnecessary; library already handles modifier detection, key naming            |
| New Tauri command  | Existing `update_settings` | No need for a new command; `keyboard_shortcuts` is just another settings field |

## Architecture Patterns

### Recommended Project Structure

```
src/
├── components/setting/
│   └── ShortcutsSection.tsx       # New settings section component
│   └── ShortcutRow.tsx            # Individual shortcut row with edit/reset
│   └── KeyRecorder.tsx            # Key recording widget (click-to-record)
├── shortcuts/
│   └── definitions.ts             # Uncomment reserved shortcuts
├── types/
│   └── setting.ts                 # Add keyboard_shortcuts to Settings
├── contexts/
│   └── SettingContext.tsx          # Add updateKeyboardShortcuts helper
├── i18n/locales/
│   ├── en-US.json                 # Add shortcuts section translations
│   └── zh-CN.json                 # Add shortcuts section translations
src-tauri/crates/
├── uc-core/src/settings/model.rs  # Add keyboard_shortcuts field to Settings
```

### Pattern 1: Settings Section Registration

**What:** Add new category to `SETTINGS_CATEGORIES` array in `settings-config.ts`
**When to use:** Adding any new settings section
**Example:**

```typescript
// In settings-config.ts, insert between 'appearance' and 'sync'
import { Command } from 'lucide-react'
import ShortcutsSection from './ShortcutsSection'

// Position: after appearance (index 1), before sync (index 2)
{
  id: 'shortcuts',
  icon: Command,
  Component: ShortcutsSection,
}
```

### Pattern 2: Settings Read/Write via SettingContext

**What:** Use `useSetting()` hook to read settings, call helper to write partial updates
**When to use:** Any settings section that reads/writes settings
**Example:**

```typescript
const { setting, loading, updateSetting } = useSetting()
const overrides = setting?.keyboard_shortcuts ?? {}

const handleOverrideChange = async (id: string, newKey: string) => {
  if (!setting) return
  const updated = { ...setting, keyboard_shortcuts: { ...overrides, [id]: newKey } }
  await updateSetting(updated)
}
```

### Pattern 3: Key Recording with useRecordHotkeys

**What:** Use the built-in recording hook from react-hotkeys-hook
**When to use:** When user clicks edit button to record a new key binding
**Example:**

```typescript
import { useRecordHotkeys } from 'react-hotkeys-hook'

const [keys, { start, stop, isRecording }] = useRecordHotkeys()
// keys is a Set<string> of pressed keys
// Convert to hotkey string: Array.from(keys).join('+')
```

### Pattern 4: Conflict Detection During Recording

**What:** Use existing `getCandidateKeyIssues` for real-time conflict feedback
**When to use:** After each key press during recording, before user confirms
**Example:**

```typescript
import { getCandidateKeyIssues, resolveShortcuts } from '@/shortcuts'

const resolved = resolveShortcuts(SHORTCUT_DEFINITIONS, currentOverrides)
const issues = getCandidateKeyIssues(resolved, {
  id: shortcutId,
  scope: shortcutScope,
  key: candidateKey,
})
// issues[].level: 'error' | 'warning' | 'info'
// issues[].message: conflict description
// issues[].relatedIds: conflicting shortcut ids
```

### Anti-Patterns to Avoid

- **Direct DOM keydown listeners for recording:** Use `useRecordHotkeys` instead -- it handles modifier normalization and cross-platform key names
- **Storing full shortcut definitions in settings:** Only store the user's key overrides (`Record<string, string | string[]>`), not copies of definition metadata
- **Mutating SHORTCUT_DEFINITIONS at runtime:** Definitions are the source of truth for defaults. Overrides are applied via `resolveShortcuts(definitions, overrides)`

## Don't Hand-Roll

| Problem               | Don't Build                  | Use Instead                                           | Why                                                                |
| --------------------- | ---------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------ |
| Key recording/capture | Custom keydown event handler | `useRecordHotkeys` from react-hotkeys-hook            | Handles modifier detection, key naming, cross-platform differences |
| Conflict detection    | Custom key comparison logic  | `getCandidateKeyIssues` from `shortcuts/conflicts.ts` | Already handles same-scope, same-layer, and cross-layer shadowing  |
| Key normalization     | Custom modifier sorting      | `normalizeHotkey` from `shortcuts/normalize.ts`       | Handles modifier aliases (cmd/meta/mod), consistent ordering       |
| Override resolution   | Custom merge logic           | `resolveShortcuts` from `shortcuts/conflicts.ts`      | Already merges defaults with overrides and handles array keys      |
| Settings persistence  | New Tauri command            | Existing `update_settings` command                    | Just add `keyboard_shortcuts` field to Settings struct             |

**Key insight:** The entire shortcut conflict detection and resolution system was designed with this settings UI in mind. The `ShortcutKeyOverrides` type, `resolveShortcuts`, and `getCandidateKeyIssues` functions exist specifically for this use case.

## Common Pitfalls

### Pitfall 1: Recording Mode Capturing Global Shortcuts

**What goes wrong:** When in recording mode, the app's own shortcuts (like Escape for navigation, Mod+Comma for settings) fire instead of being captured
**Why it happens:** `useShortcut` hooks are always active for the current scope
**How to avoid:** During recording mode, either: (a) disable the shortcut being edited via its `enabled` prop, or (b) use `stopPropagation`/`preventDefault` aggressively in the recorder. Note that `useRecordHotkeys` likely handles `preventDefault` internally.
**Warning signs:** Pressing Escape during recording navigates away instead of canceling recording

### Pitfall 2: Comma Delimiter Conflict in react-hotkeys-hook

**What goes wrong:** Key combos containing comma (like `mod+,`) get split into multiple keys
**Why it happens:** react-hotkeys-hook uses comma as key separator by default
**How to avoid:** The codebase already handles this -- `useShortcut` uses `delimiter: '§'` to avoid comma conflicts. Ensure the recorder output is compatible with this convention.
**Warning signs:** `mod+comma` being interpreted as two separate shortcuts

### Pitfall 3: useRecordHotkeys Key Format Mismatch

**What goes wrong:** Keys recorded by `useRecordHotkeys` don't match the format expected by `normalizeHotkey`
**Why it happens:** `useRecordHotkeys` returns a `Set<string>` of individual key tokens (e.g., `{'meta', 's'}`), not a combined hotkey string (e.g., `'meta+s'`)
**How to avoid:** Convert the recorded Set to a hotkey string by joining with `+`, then run through `normalizeHotkey` to ensure consistent format
**Warning signs:** Conflict detection not finding matches for recorded keys

### Pitfall 4: serde(default) Missing on New Settings Field

**What goes wrong:** Existing settings files fail to deserialize after adding `keyboard_shortcuts` field
**Why it happens:** Without `#[serde(default)]`, serde requires the field to exist in stored JSON
**How to avoid:** Use `#[serde(default)]` on the new `keyboard_shortcuts` field in Rust Settings struct
**Warning signs:** App crash on startup for existing users

### Pitfall 5: Not Propagating Overrides to Active Shortcuts

**What goes wrong:** User changes key binding in settings but the shortcut doesn't take effect until restart
**Why it happens:** `useShortcut` hooks use hardcoded key values from definitions, not the overrides from settings
**How to avoid:** After saving overrides to settings, the `useShortcut` calls need to read the effective key (definition default merged with override). This requires `ShortcutProvider` or a hook to expose resolved keys.
**Warning signs:** Changed shortcut only works after app restart

## Code Examples

### Adding keyboard_shortcuts to Rust Settings

```rust
// In src-tauri/crates/uc-core/src/settings/model.rs
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub sync: SyncSettings,
    #[serde(default)]
    pub retention_policy: RetentionPolicy,
    #[serde(default)]
    pub security: SecuritySettings,
    #[serde(default)]
    pub pairing: PairingSettings,
    // New field: key overrides for keyboard shortcuts
    #[serde(default)]
    pub keyboard_shortcuts: HashMap<String, serde_json::Value>,
}
```

### Adding keyboard_shortcuts to TypeScript Settings

```typescript
// In src/types/setting.ts
export interface Settings {
  schema_version: number
  general: GeneralSettings
  sync: SyncSettings
  retention_policy: RetentionPolicy
  security: SecuritySettings
  pairing: PairingSettings
  keyboard_shortcuts?: Record<string, string | string[]>
}
```

### Uncommenting Definitions in definitions.ts

```typescript
// Uncomment all reserved shortcuts in SHORTCUT_DEFINITIONS array:
// clipboard.selectAll, clipboard.delete, clipboard.favorite, clipboard.copy
// nav.dashboard, nav.devices
// search.focus
// modal.close
```

### Key Recorder Component Pattern

```typescript
import { useRecordHotkeys } from 'react-hotkeys-hook'
import { normalizeHotkey } from '@/shortcuts/normalize'

function KeyRecorder({ onRecord, onCancel }: { onRecord: (key: string) => void; onCancel: () => void }) {
  const [keys, { start, stop, isRecording }] = useRecordHotkeys()

  useEffect(() => {
    start() // Begin recording immediately when mounted
  }, [start])

  const recordedKey = useMemo(() => {
    if (keys.size === 0) return ''
    return normalizeHotkey(Array.from(keys).join('+'))
  }, [keys])

  // Esc cancels recording
  useEffect(() => {
    if (keys.has('escape')) {
      stop()
      onCancel()
    }
  }, [keys, stop, onCancel])

  return (
    <div className="flex items-center gap-2">
      <kbd className="...">{recordedKey || 'Press keys...'}</kbd>
      <Button size="sm" onClick={() => { stop(); onRecord(recordedKey) }}>Confirm</Button>
    </div>
  )
}
```

### Shortcut Badge Display

```typescript
// Display modifier keys with platform-appropriate symbols
const MODIFIER_SYMBOLS: Record<string, string> = {
  cmd: navigator.platform.includes('Mac') ? '⌘' : 'Ctrl',
  ctrl: '⌃',
  alt: navigator.platform.includes('Mac') ? '⌥' : 'Alt',
  shift: '⇧',
}

function KeyBadge({ hotkey }: { hotkey: string }) {
  const parts = hotkey.split('+')
  return (
    <div className="flex items-center gap-0.5">
      {parts.map(part => (
        <kbd key={part} className="px-1.5 py-0.5 rounded bg-muted text-xs font-mono">
          {MODIFIER_SYMBOLS[part] ?? part.toUpperCase()}
        </kbd>
      ))}
    </div>
  )
}
```

## State of the Art

| Old Approach                    | Current Approach                                    | When Changed         | Impact                                                      |
| ------------------------------- | --------------------------------------------------- | -------------------- | ----------------------------------------------------------- |
| Custom keyboard listeners       | react-hotkeys-hook useRecordHotkeys                 | v4+ of library       | Built-in recording support, no custom implementation needed |
| Separate settings API per field | Unified `update_settings` with full Settings object | Current architecture | No new Tauri command needed for keyboard_shortcuts          |

## Open Questions

1. **useRecordHotkeys key format details**
   - What we know: Returns a `Set<string>` of pressed keys
   - What's unclear: Exact key naming conventions (e.g., does it return "meta" or "cmd" for the Command key on macOS?)
   - Recommendation: Test at implementation time, use `normalizeHotkey` to standardize output

2. **Live override propagation to active shortcuts**
   - What we know: Current `useShortcut` hooks use hardcoded keys from call sites (e.g., `key: 'esc'` in SettingsPage)
   - What's unclear: Whether we should refactor all `useShortcut` calls to read from resolved overrides, or handle this in a later phase
   - Recommendation: For this phase, focus on persistence and the settings UI. Wire up override consumption in the shortcut system as part of this phase since the infrastructure (`resolveShortcuts`) already exists. The `useShortcut` hook can accept the override-resolved key.

3. **HashMap vs BTreeMap for Rust keyboard_shortcuts**
   - What we know: Serde serializes both to JSON object. `ShortcutKeyOverrides` on TS side is `Record<string, string | string[]>`
   - What's unclear: Whether ordering matters
   - Recommendation: Use `HashMap<String, serde_json::Value>` -- ordering doesn't matter, and `serde_json::Value` handles both string and array values cleanly

## Sources

### Primary (HIGH confidence)

- Project source code: `src/shortcuts/` module -- definitions, conflicts, normalize, layers (all read directly)
- Project source code: `src/components/setting/` -- settings-config, SettingGroup, SettingRow patterns (all read directly)
- Project source code: `src-tauri/crates/uc-core/src/settings/model.rs` -- Rust Settings struct (read directly)
- Project source code: `src/types/setting.ts` -- TypeScript Settings interface (read directly)
- Project source code: `src/contexts/SettingContext.tsx` -- settings read/write pattern (read directly)
- react-hotkeys-hook package.json: version 5.2.1 confirmed (read directly)

### Secondary (MEDIUM confidence)

- [react-hotkeys-hook docs](https://react-hotkeys-hook.vercel.app/) -- `useRecordHotkeys` API (via WebSearch, consistent with library's purpose)
- [npm react-hotkeys-hook](https://www.npmjs.com/package/react-hotkeys-hook) -- feature confirmation

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all libraries already installed and in use
- Architecture: HIGH - follows exact existing settings section patterns, shortcut infrastructure pre-built
- Pitfalls: HIGH - identified from direct code analysis of existing patterns and known react-hotkeys-hook behaviors

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable -- no external dependencies changing)
