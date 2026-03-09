# Phase 17: Dynamic Theme Switching with Shadcn Preset Import and Enhanced Color Preview - Research

**Researched:** 2026-03-08
**Domain:** Frontend theming architecture (React + Tailwind v4 CSS variables + shadcn preset conventions)
**Confidence:** HIGH

## Summary

Phase 17 should migrate theme color definitions from static CSS theme files to TypeScript preset objects, then apply them at runtime through CSS variable injection. This keeps the current mode logic (`light` / `dark` / `system`) and settings persistence (`theme_color`) intact while enabling richer preset management and improved swatch preview UX.

The current architecture already has a clean integration point in `SettingProvider`: it computes effective mode and writes `data-theme` on `<html>`. The safest implementation path is to keep this ownership and extend it with a dedicated theme engine utility that injects token variables (`--background`, `--primary`, etc.) from preset definitions. `AppearanceSection` then consumes the same preset metadata to render multi-dot preview swatches, avoiding duplicated color sources.

The migration should be done in two waves:

1. Theme runtime engine + CSS migration (remove static theme imports and move token definitions to TS).
2. Appearance UI swatch upgrade + regression tests.

<user_constraints>

## User Constraints (from 17-CONTEXT.md)

### Locked Decisions

- Theme source is a built-in curated preset list; no user custom import/editor.
- Theme definitions move from `src/styles/themes/*.css` into TS objects.
- Runtime applies theme variables to `document.documentElement`.
- Existing mode behavior (`light` / `dark` / `system`) is preserved.
- Existing persistence remains (`updateGeneralSetting({ theme_color })` -> backend `GeneralSettings.theme_color`).
- Appearance swatch changes from single solid dot to 3-4 dot color group.
- No hover-to-preview global theme switching.
- Theme switching includes smooth color transition (~200ms).

### Claude's Discretion

- Exact preset composition (curated set size and members).
- Internal TS object schema for theme presets and tokens.
- Dot color mapping for preview swatches.
- Exact transition CSS properties and timing function.
- Minimal startup fallback strategy to avoid initial flash during JS boot.

### Deferred Ideas (OUT OF SCOPE)

- User-defined/custom themes and URL/JSON import.
- Theme grouping/metadata UI.
- Theme name i18n.
- Layout/typography structure redesign.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID     | Description                                                                                                                                                  |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| P17-01 | Theme presets are defined as TS objects (light/dark token sets), not static per-theme CSS files.                                                             |
| P17-02 | Runtime applies selected theme preset tokens to root CSS variables while preserving existing mode semantics (`light` / `dark` / `system`).                   |
| P17-03 | `globals.css` no longer imports per-theme CSS files; concrete per-theme color ownership moves to runtime injection (allowing only minimal startup fallback). |
| P17-04 | Appearance section renders built-in preset options with 3-4 dot color previews sourced from preset tokens.                                                   |
| P17-05 | Theme selection persists through existing settings flow (`theme_color`), and persisted value is reapplied on startup.                                        |
| P17-06 | Theme color switch is animated with smooth transition (~200ms) on key color variables.                                                                       |
| P17-07 | Automated tests cover runtime token injection, fallback behavior, and appearance swatch interaction/persistence contract.                                    |

</phase_requirements>

## Standard Stack

### Core

| Library / Module                  | Version / Origin                       | Purpose                                             |
| --------------------------------- | -------------------------------------- | --------------------------------------------------- |
| React 18 + TypeScript             | existing project stack                 | Settings UI and context orchestration               |
| Tailwind CSS v4 (`@theme inline`) | `src/styles/globals.css` + shadcn info | Semantic token consumption through CSS variables    |
| shadcn radix-nova preset style    | `components.json` / `shadcn info`      | Theme token conventions and semantic color usage    |
| `SettingProvider`                 | `src/contexts/SettingContext.tsx`      | Existing authoritative place to apply mode + theme  |
| Settings command flow             | `get_settings` / `update_settings`     | Persistence of `theme_color` without schema changes |

### Supporting

| Module                                         | Purpose                          | When to Use                                                          |
| ---------------------------------------------- | -------------------------------- | -------------------------------------------------------------------- |
| `src/constants/theme.ts`                       | Theme option source              | Upgrade to include full preset token definitions + preview metadata  |
| `src/components/setting/AppearanceSection.tsx` | Theme picker UI                  | Upgrade preview from single dot to multi-dot swatches                |
| `src/styles/globals.css`                       | Global token bridge              | Remove static theme imports and add transition/fallback behavior     |
| `src/styles/themes/*.css`                      | Current static theme definitions | Migration source only; should be removed from active theme ownership |

## Architecture Patterns

### Pattern 1: Single Source of Theme Truth (TS presets)

Define each preset in TS with explicit `light` and `dark` token maps, then derive:

- Runtime injection data.
- Appearance swatch preview colors.

This avoids drift between UI preview and real applied colors.

### Pattern 2: Mode and Color Responsibilities Stay Split

Keep mode class handling in `SettingProvider` (`light`/`dark`/`system`) and add token injection as a separate step in the same effect. This preserves existing behavior and minimizes regression risk.

### Pattern 3: Minimal CSS Fallback + Runtime Ownership

`globals.css` should keep only minimal boot fallback for startup flash prevention, while final theme tokens come from runtime preset injection. Remove direct `@import './themes/*.css'` ownership.

### Pattern 4: Semantic Transition Only

Transition only variable-driven color surfaces (background/border/text/fill/stroke) with ~200ms easing to avoid animating layout or expensive properties.

## Don't Hand-Roll

| Problem                       | Don't Build                                      | Use Instead                                                  |
| ----------------------------- | ------------------------------------------------ | ------------------------------------------------------------ |
| Theme storage                 | New backend schema/table                         | Existing `theme_color: Option<String>` in settings           |
| Mode management               | New mode state machine                           | Existing `SettingProvider` mode logic with `matchMedia`      |
| Per-theme styling duplication | Separate UI swatch colors hardcoded in component | Derive swatch dots from same preset token source             |
| Theme runtime switching       | Component-level ad hoc style mutations           | Centralized `applyThemePreset(themeName, mode, root)` helper |

## Common Pitfalls

### Pitfall 1: Inconsistent preview vs applied theme

If swatch colors are hardcoded independently from runtime tokens, users will see mismatched previews.

Mitigation: generate swatch dots from preset metadata in the same module as token sets.

### Pitfall 2: Flash of wrong colors at startup

Removing static CSS theme blocks can cause startup flash before settings load.

Mitigation: keep minimal neutral fallback in `globals.css`, then apply persisted preset immediately in `SettingProvider` effect.

### Pitfall 3: Breaking `system` mode listeners

If refactor merges mode and preset logic incorrectly, system theme changes may stop applying.

Mitigation: keep existing `matchMedia` listener lifecycle; only append injection step after effective mode is computed.

### Pitfall 4: Over-animating transitions

Animating too many properties or large scope can feel sluggish.

Mitigation: restrict transition to color-related properties and keep duration short (~200ms).

## Validation Architecture

### Test Framework

- Frontend: Vitest + Testing Library.
- Backend: unchanged for this phase (settings schema and commands are reused as-is).

### Phase Requirements -> Test Map

| Requirement | Test Target                                                                                                      | Test Type                  |
| ----------- | ---------------------------------------------------------------------------------------------------------------- | -------------------------- |
| P17-01      | Theme preset module exports complete light/dark token maps for each built-in preset                              | unit                       |
| P17-02      | Theme engine applies token variables for selected preset + mode to document root                                 | unit                       |
| P17-03      | `globals.css` no longer imports `src/styles/themes/*.css`                                                        | static assertion / unit    |
| P17-04      | Appearance swatch renders 3-4 dots per preset and selected state is highlighted                                  | component                  |
| P17-05      | Clicking swatch calls `updateGeneralSetting({ theme_color })`; default fallback works when `theme_color` missing | component                  |
| P17-06      | Transition CSS exists on key color properties                                                                    | static assertion / unit    |
| P17-07      | Combined tests above run green via `bun run test --run`                                                          | integration-at-phase-level |

### Sampling Rate

- After each task commit: `bun run test --run`
- After each plan wave: `bun run test --run && bun run build`
- Before `/gsd:verify-work`: `bun run test --run && bun run build`

### Wave 0 Gaps

- No existing dedicated tests for `AppearanceSection` theme swatch interactions.
- No existing dedicated tests for runtime token injection behavior in `SettingProvider`.

## Sources

### Primary (HIGH confidence)

- `.planning/phases/17-dynamic-theme-switching-with-shadcn-preset-import-and-enhanced-color-preview/17-CONTEXT.md`
- `src/contexts/SettingContext.tsx`
- `src/components/setting/AppearanceSection.tsx`
- `src/constants/theme.ts`
- `src/styles/globals.css`
- `src/styles/themes/zinc.css`
- `src/styles/themes/catppuccin.css`
- `src/styles/themes/t3chat.css`
- `src/styles/themes/claude.css`
- `src/types/setting.ts`
- `src-tauri/crates/uc-core/src/settings/model.rs`

### Secondary (MEDIUM confidence)

- shadcn project metadata from `bunx --bun shadcn@latest info --json` (radix-nova, tailwind v4, global css path)

## Metadata

- Scope: frontend-only behavior + CSS token migration
- Backend schema changes: none
- Risk level: medium (startup flash + mode regression risks)
- Recommended plan count: 2
