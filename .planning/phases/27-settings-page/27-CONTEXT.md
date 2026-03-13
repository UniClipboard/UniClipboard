# Phase 27: Keyboard Shortcuts Settings - Context

**Gathered:** 2026-03-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Add a "Shortcuts" settings category to the Settings page where users can view all keyboard shortcuts (grouped by scope), customize key bindings via a key recorder, see real-time conflict detection, and persist/reset custom bindings through the existing Rust settings system. Also activate the currently-commented-out shortcut definitions.

</domain>

<decisions>
## Implementation Decisions

### Settings Entry Point

- New independent category "Shortcuts" in Settings sidebar, positioned after "Appearance" and before "Sync"
- Uses lucide `Command` icon
- Title: "Shortcuts"

### Shortcut List Scope

- Expose ALL shortcuts — activate the ~8 commented-out shortcut definitions in `definitions.ts` (selectAll, delete, favorite, copy, navigation 1/2, search, modal close)
- Combined with the 2 already active shortcuts (esc clear selection, mod+comma settings), this gives ~10 total shortcuts
- Group by scope (global, clipboard, settings, devices, modal) with section headers

### Key Recording Interaction

- Click-to-record key binding capture: user clicks an edit button, then presses desired key combination
- Esc cancels recording mode
- Real-time conflict detection using existing `getCandidateKeyIssues` from `conflicts.ts` during recording
- When conflict detected: show conflict info inline, user can choose to confirm (override — clears the conflicting binding) or cancel

### Persistence & Reset

- Store custom key overrides in Rust settings system (new `keyboard_shortcuts` field in settings, consistent with existing settings architecture)
- Read/write via existing `get_settings` / `update_settings` Tauri commands
- Per-shortcut reset button (↺ icon) next to each shortcut to restore individual default
- "Reset All Shortcuts" button at the bottom of the section to restore all defaults
- Override format matches existing `ShortcutKeyOverrides` type: `Record<string, string | string[]>`

### Claude's Discretion

- Exact layout and spacing of the shortcut list rows
- Animation/transition for recording state
- How to display modifier key symbols (⌘/⌃/⌥/⇧ vs text)
- Whether to show scope descriptions or just scope names as section headers
- Error state and edge case handling

</decisions>

<specifics>
## Specific Ideas

- The preview mockup resonated: each row shows action description, current key binding displayed as keyboard shortcut badge, and an edit button
- Conflict resolution should feel non-destructive — user explicitly chooses to override, never auto-clears without consent
- Reset button only appears when a shortcut has been modified from its default

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `shortcuts/definitions.ts`: `ShortcutDefinition` type and `SHORTCUT_DEFINITIONS` array — need to uncomment reserved shortcuts
- `shortcuts/conflicts.ts`: `resolveShortcuts()`, `analyzeShortcutConflicts()`, `getCandidateKeyIssues()` — full conflict detection ready to use
- `shortcuts/normalize.ts`: `normalizeHotkey()` — key normalization for comparison
- `react-hotkeys-hook`: Already a dependency, used by `useShortcut` hook
- `settings-config.ts`: `SETTINGS_CATEGORIES` array — add new entry here
- Existing `ShortcutKeyOverrides` type in `conflicts.ts` — matches the planned override storage format

### Established Patterns

- Settings sections are React components registered in `SETTINGS_CATEGORIES` array with id, icon, and Component
- Settings read/write via `get_settings` / `update_settings` Tauri commands
- Shortcut context uses layer-based priority: modal > page > global
- `useShortcutScope` hook for page-level scope activation

### Integration Points

- `settings-config.ts`: Add new "shortcuts" category entry between "appearance" and "sync"
- `definitions.ts`: Uncomment reserved shortcut definitions to activate them
- Rust settings: Add `keyboard_shortcuts` field to settings model
- `ShortcutProvider` / `useShortcut`: Need to consume overrides from settings and apply them to definitions

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 27-settings-page_
_Context gathered: 2026-03-13_
