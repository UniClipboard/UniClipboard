---
phase: 17-dynamic-theme-switching-with-shadcn-preset-import-and-enhanced-color-preview
plan: '01'
subsystem: frontend-theme
name: Runtime theme preset engine and SettingProvider integration
one_liner: Runtime theme presets with light/dark token maps and SettingProvider-driven CSS variable injection
requires: [P17-01, P17-02, P17-03, P17-05, P17-06]
provides:
  - theme-presets-registry
  - runtime-theme-injection
  - css-theme-migration
tech_stack:
  added:
    - custom TS theme preset registry
  patterns:
    - runtime CSS variable injection
    - TDD-style vitest coverage for theming
key_files:
  created:
    - src/lib/theme-engine.ts
    - src/lib/__tests__/theme-engine.test.ts
    - src/contexts/__tests__/SettingContext.theme.test.tsx
    - src/styles/__tests__/theme-migration.test.ts
  modified:
    - src/constants/theme.ts
    - src/contexts/SettingContext.tsx
    - src/styles/globals.css
commits:
  - hash: 83becf21
    message: feat(17-01): add theme engine preset registry and tests
  - hash: 85d745b9
    message: feat(17-01): integrate runtime theme injection and CSS migration
metrics:
  duration: ~12h (calendar)
  completed_at: 2026-03-08T11:43:03Z
---

# Phase 17 Plan 01: Runtime theme preset engine and SettingProvider integration

## Overview

This plan introduced a runtime theme preset engine with explicit light/dark token maps, integrated it into the React SettingProvider, and migrated color ownership from static CSS theme files to TypeScript presets and root CSS variable injection.

## What Was Implemented

### 1. Theme preset registry and helpers

**Files:**

- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/lib/theme-engine.ts
- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/lib/**tests**/theme-engine.test.ts
- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/constants/theme.ts

Changes:

- Defined `ThemeTokens`, `ThemePreset`, and `ThemeMode` types to represent required theme tokens for both light and dark modes.
- Migrated existing CSS theme definitions for `zinc`, `catppuccin`, `t3chat`, and `claude` into TypeScript objects (`zincLight`, `zincDark`, etc.) preserving the OKLCH values from the legacy CSS files.
- Built a `PRESETS` registry keyed by theme name, including:
  - `name`
  - `accentColor` (for legacy single-color usage)
  - `previewDots` (3–4 representative colors per preset)
  - `light` / `dark` token maps.
- Implemented `DEFAULT_THEME_COLOR = 'zinc'` as the canonical default and exported it from the theme engine.
- Added helper functions:
  - `getPresetOrDefault(themeName)` to gracefully fall back to the default preset when the name is null/undefined/unknown.
  - `getTokens(themeName, mode)` to select light/dark tokens.
  - `applyThemePreset(themeName, mode, root)` to write all relevant CSS variables onto `root.style` and keep `data-theme` in sync with the resolved preset.
  - `getThemePreviewDots(themeName, mode)` to return 3–4 preview colors per preset, falling back to derived tokens if necessary.
- Exported `themePresets` for consumers that need access to the registry.

`src/constants/theme.ts` was refactored into a thin adapter over the presets:

- Imports `themePresets` and `DEFAULT_THEME_COLOR`.
- Defines `ThemeColorOption` with `name`, `color`, and `previewDots`.
- Derives `THEME_COLORS` by mapping over `themePresets`, ensuring no duplicated hard-coded preview hex values.

Tests (`theme-engine.test.ts`):

- Verifies that applying a known preset in light mode sets representative CSS vars (`--background`, `--primary`, `--border`).
- Confirms that light and dark backgrounds for the same preset differ as expected.
- Ensures unknown preset names fall back to the default preset and `data-theme` reflects the default.
- Validates that `getThemePreviewDots` returns 3–4 non-empty strings and that unknown theme names share the default dots.

### 2. SettingProvider integration and CSS ownership migration

**Files:**

- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/contexts/SettingContext.tsx
- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/styles/globals.css
- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/contexts/**tests**/SettingContext.theme.test.tsx
- /Users/mark/conductor/workspaces/uniclipboard-desktop/new-york-v1/src/styles/**tests**/theme-migration.test.ts

#### SettingProvider runtime theme injection

In `SettingContext.tsx`:

- Imported `applyThemePreset` from the new theme engine.
- Kept the existing mode resolution logic (`light` / `dark` / `system`) and `matchMedia` handling intact.
- Updated the theme effect to:
  - Compute `theme` and `themeColor` (fallback to `DEFAULT_THEME_COLOR` when unset).
  - Remove existing `light`/`dark` classes.
  - Resolve `resolvedMode` as either explicit `theme` or `system`-derived value.
  - Add the resolved mode class to `documentElement`.
  - Call `applyThemePreset(themeColor, resolvedMode, root)` to inject all CSS variables and synchronize `data-theme` with the effective preset name.

This preserves existing mode semantics while moving color token ownership to the TS registry and runtime injection.

#### globals.css migration

In `globals.css`:

- Removed legacy per-theme imports:
  - `@import './themes/zinc.css';`
  - `@import './themes/catppuccin.css';`
  - `@import './themes/t3chat.css';`
  - `@import './themes/claude.css';`
- Retained the minimal `:root` and `.dark` token definitions, which now serve as startup/fallback values prior to React hydration.
- Added a smooth color transition on the `body` element:
  ```css
  body {
    background-color: transparent;
    @apply bg-background text-foreground;
    transition:
      background-color 200ms ease,
      color 200ms ease,
      border-color 200ms ease;
  }
  ```
  This yields ~200ms transitions for key color-related properties when theme presets change.

#### Tests for SettingProvider and CSS migration

`SettingContext.theme.test.tsx`:

- Uses a mocked `useSetting` hook to provide a stable `setting.general` with:
  - `theme: 'light'`
  - `theme_color: DEFAULT_THEME_COLOR`
  - `language: 'en'`
- Stubs `window.matchMedia` in `beforeEach` to avoid jsdom limitations.
- Verifies that on mount:
  - `setting.general.theme_color` equals `DEFAULT_THEME_COLOR`.
  - `documentElement.dataset.theme` is set to `DEFAULT_THEME_COLOR`, confirming that the persisted theme color is applied.
- Simulates `theme_color` being set to `null` and asserts that `data-theme` falls back to `DEFAULT_THEME_COLOR` without throwing.
- Creates a dark `matchMedia` mock to assert that the `system`-mode behavior still results in the `dark` class being present when appropriate.

`theme-migration.test.ts`:

- Reads `globals.css` as text and asserts:
  - The file no longer contains `@import './themes/` (static per-theme CSS imports have been removed).
  - The CSS includes a `transition` definition with `background-color 200ms` to confirm the presence of the intended color transition.

### 3. Verification

Commands executed:

- `bun run test --run src/lib/__tests__/theme-engine.test.ts`
- `bun run test --run src/contexts/__tests__/SettingContext.theme.test.tsx src/styles/__tests__/theme-migration.test.ts`
- `bun run test --run src/lib/__tests__/theme-engine.test.ts src/contexts/__tests__/SettingContext.theme.test.tsx src/styles/__tests__/theme-migration.test.ts`
- `bun run build`

All plan-specific tests pass, and the production build succeeds. The warnings seen in vitest output are from Tauri API calls in the jsdom environment but do not affect test outcomes (tests are passing and expectations are met).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Circular dependency between theme constants and theme engine**

- **Found during:** Task 1 while making `THEME_COLORS` derive from the TS preset registry.
- **Issue:** Importing `DEFAULT_THEME_COLOR` from `@/constants/theme` into `theme-engine.ts` while also importing `themePresets` into `constants/theme.ts` produced a circular dependency and runtime error (`Object.values` on undefined).
- **Fix:**
  - Moved `DEFAULT_THEME_COLOR` into `theme-engine.ts` as the canonical source.
  - Updated `constants/theme.ts` to import both `themePresets` and `DEFAULT_THEME_COLOR` from `theme-engine.ts`.
  - Introduced an intermediate `presetList` variable to hold `Object.values(themePresets)` before mapping into `THEME_COLORS`.
- **Files modified:**
  - src/lib/theme-engine.ts
  - src/constants/theme.ts
- **Commit:** 83becf21

**2. [Rule 3 - Blocking] Vitest + jsdom environment gaps for `matchMedia` and Tauri APIs**

- **Found during:** Task 2 while adding SettingProvider integration tests.
- **Issue:**
  - `window.matchMedia` is not implemented in jsdom by default, causing `vi.spyOn(window, 'matchMedia')` to fail.
  - Tauri event/command APIs (`@tauri-apps/api/event` and `invoke`) raised TypeErrors in the test environment.
- **Fix:**
  - Added a manual `matchMedia` stub in `beforeEach` using a typed cast on `window` to avoid TypeScript errors.
  - Left Tauri-related warnings as non-fatal (they log to stderr but tests assert only on DOM behavior and pass successfully), keeping the tests focused on theme behavior rather than backend wiring.
- **Files modified:**
  - src/contexts/**tests**/SettingContext.theme.test.tsx
- **Commit:** 85d745b9

**3. [Rule 3 - Blocking] Node type resolution in tests for CSS migration**

- **Found during:** Task 2 while asserting on `globals.css` contents.
- **Issue:** Using `fs`, `path`, and `url` in the test required Node typings, which conflicted with the existing tsconfig setup.
- **Fix:**
  - Implemented the CSS migration test as a Node-script-style test file with `// @ts-nocheck` at the top to bypass Node type requirements.
  - Kept the test implementation minimal: read `globals.css` via `fs.readFileSync` and assert import removal and transition presence.
- **Files modified:**
  - src/styles/**tests**/theme-migration.test.ts
  - tsconfig.json (types restored to default frontend configuration)
- **Commit:** 85d745b9

### Auth/Environment Warnings

- Vitest runs emitted warnings and errors from Tauri APIs (`listen`, `invoke`) when `SettingProvider` mounted in the jsdom environment. These are expected given the absence of a real Tauri context. Tests are written to assert only on DOM and theme behavior, and all expectations still pass. No additional mocks were introduced beyond what was required for theme behavior.

## Requirements Mapping

Covered requirements:

- **P17-01:** Theme presets defined in TS with clear light/dark maps and metadata via `theme-engine.ts`.
- **P17-02:** SettingProvider now uses `applyThemePreset` to inject theme tokens at runtime based on `theme` and `theme_color`.
- **P17-03:** `globals.css` no longer imports static per-theme CSS files; theme ownership is centralized in the TS registry.
- **P17-05:** Persisted `theme_color` is applied on mount, and unknown/null values fall back to `DEFAULT_THEME_COLOR` (tested).
- **P17-06:** Body now has a ~200ms color transition on background, text, and border colors.

## Self-Check

- `src/lib/theme-engine.ts` exists and exports the documented helpers and registry.
- `src/lib/__tests__/theme-engine.test.ts` exists and passes.
- `src/contexts/SettingContext.tsx` applies mode and then injects theme tokens via `applyThemePreset`.
- `src/styles/globals.css` does not contain `@import './themes/` and includes the new transition.
- `src/contexts/__tests__/SettingContext.theme.test.tsx` exists and passes.
- `src/styles/__tests__/theme-migration.test.ts` exists and passes.
- `bun run test --run src/lib/__tests__/theme-engine.test.ts src/contexts/__tests__/SettingContext.theme.test.tsx src/styles/__tests__/theme-migration.test.ts` passes.
- `bun run build` passes.

## Self-Check: PASSED
