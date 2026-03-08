# Phase 17: Dynamic Theme Switching with Shadcn Preset Import and Enhanced Color Preview - Context

**Gathered:** 2026-03-08
**Status:** Ready for planning

<domain>
## Phase Boundary

Upgrade the existing theme system so users can switch between multiple color themes derived from shadcn presets, with an improved visual preview of each theme. The phase focuses on:

- Using shadcn-style color presets as the source of multiple themes
- Migrating theme definitions from CSS files to JS/TS objects with runtime CSS-variable injection
- Enhancing the theme color preview UI in the Settings → Appearance section
- Preserving existing theme mode handling (light / dark / system) and backend persistence of `theme_color`

Out of scope:

- Arbitrary user-defined/custom themes or URL/JSON-based theme import
- Changing layout, typography, or component structure — themes only change colors
- Backend schema changes beyond the existing `theme_color` string in settings

</domain>

<decisions>
## Implementation Decisions

### Theme source & structure

- Themes are provided primarily via a **preset list**, not user-imported themes.
- We will use shadcn-related resources to choose a **curated set** of presets that provide good visual diversity (e.g., zinc + a handful of vibrant schemes), but not necessarily every single shadcn preset.
- Theme **definitions move from CSS files to JS/TS objects**:
  - Each theme is defined as a TS object mapping logical tokens (e.g. `background`, `foreground`, `card`, `primary`, `accent`, etc.) to oklch color values.
  - At runtime, the app injects these values into `document.documentElement` as CSS variables (e.g. `--background`, `--primary`).
- Existing theme color options (`zinc`, `catppuccin`, `t3chat`, `claude`) are **migrated from their CSS files into JS/TS theme objects**.
- The theme system continues to use:
  - `data-theme` attribute on `<html>` to select which theme object to apply.
  - `class="light"` / `class="dark"` on `<html>` for mode (light/dark/system) selection.

### Theme preview (color preview enhancement)

- The preview for each theme color becomes a **multi-dot color group** instead of a single solid circle.
- Each theme swatch shows **3–4 small circles** representing key colors from the theme; exact mapping is a Claude decision (see "Claude's Discretion").
- No live global preview on hover:
  - **No temporary global theme switch on hover** (no hover-based full-app preview).
  - The theme is applied globally only when the user clicks to select.

### Theme management

- Users can only choose from a **built-in preset list** of themes:
  - **No custom theme creation** by users in this phase.
  - **No URL/JSON import** for arbitrary themes in this phase.
- The theme list remains a **grid layout** (similar to the current 5-column grid), adjusted as necessary for the final theme count.
- Themes are **not grouped** in the UI (no "official" vs "community" sections), since all themes are built-in presets.
- Theme names remain **plain English identifiers** (e.g. `zinc`, `catppuccin`, `rose`), with **no i18n translations** for theme names.
- Theme definitions (names + color values) are stored as **frontend constants** in TS modules.
- The backend continues to store only the **selected theme name** (`theme_color` string) in `GeneralSettings`; it does **not** store full theme definitions.
- `globals.css` **no longer owns concrete theme color values**:
  - The existing `:root` and `.dark` color-variable blocks are removed or stripped down so that **all theme color variables come from JS runtime injection**, not from static CSS.

### Switching experience

- Theme switching uses a **smooth CSS-based color transition** instead of an instant jump:
  - Key CSS variables (e.g. background, card, primary, accent, border, etc.) should have `transition` applied so the change animates over ~200ms.
  - The transition should feel responsive but not sluggish.
- Each theme provides **both light and dark variants**:
  - For a given theme name (e.g. `catppuccin`), there is a light-color set and a dark-color set.
  - Which variant is applied depends on the **current theme mode** (`light` / `dark` / `system`): the mode logic in `SettingContext` stays in charge of adding `light` / `dark` class, and the theme injection logic chooses the appropriate variant.
- Theme selection remains **persisted via settings**:
  - On change, the frontend continues to call `updateGeneralSetting({ theme_color: newThemeColor })`.
  - The backend stores this value in `GeneralSettings` so the theme is preserved across app restarts.

### Claude's Discretion

During planning/implementation, Claude can decide the following without asking the user again:

- **Preset set composition**:
  - Exactly which shadcn-like presets to include (beyond the existing four), provided they give a good spread of hues and are not overwhelming in number.
- **Color mapping for preview dots**:
  - Which tokens map to the 3–4 dots (e.g. `primary`, `accent`, `background`, `card`).
  - Exact ordering and sizing of the dots inside the swatch.
- **Internal theme representation**:
  - Structure of the TS theme object type (e.g. grouping by mode, `light`/`dark` fields, and which tokens are mandatory).
  - Implementation details of the runtime injection utility (e.g. a dedicated `applyTheme(themeName, mode)` helper that sets CSS variables).
- **Transition details**:
  - Exact `transition` CSS (properties to animate, duration, easing).
  - Which elements receive the transitions (e.g. root, body, main containers) as long as performance remains acceptable.
- **Migration mechanics**:
  - How to gradually migrate away from `globals.css` theme variables while preventing a flash of incorrect colors.
  - Whether to keep minimal fallback values in CSS versus relying purely on JS injection once the app has booted.

</decisions>

<specifics>
## Specific Ideas

- Theme **only affects colors**, not spacing, typography, or component layout — this keeps visual identity coherent while still allowing color customization.
- Multi-dot swatches should be **visually compact**, similar in footprint to the current single-dot design, to avoid blowing up the layout.
- The curated preset set should aim for:
  - A neutral/gray baseline (zinc-style).
  - A few colorful but balanced options (e.g. blue, green, violet, rose).
  - A small number of more opinionated themes (like `catppuccin`, `claude`) for personality.

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `src/constants/theme.ts`:
  - Defines `THEME_COLORS` and `DEFAULT_THEME_COLOR` used by the Appearance section.
  - This is the natural place (or the seed) to evolve into a richer theme-definition module.
- `src/contexts/SettingContext.tsx`:
  - Owns `SettingProvider` and all settings load/save behavior.
  - `applyTheme` logic already handles:
    - Reading `setting.general.theme` (`light`/`dark`/`system`).
    - Reading `setting.general.theme_color` (or `DEFAULT_THEME_COLOR`).
    - Toggling `light`/`dark` classes on `document.documentElement` based on system preference.
    - Setting `data-theme` attribute on `<html>`.
  - This is the **primary integration point** for injecting new theme variable sets.
- `src/components/setting/AppearanceSection.tsx`:
  - Renders theme mode options (Light/Dark/System) and theme color options using `THEME_COLORS`.
  - Calls `updateGeneralSetting` to persist `theme` and `theme_color`.
  - The theme-color portion is the primary target for replacing the single-dot preview with multi-dot swatches and possibly richer labels.
- `src/styles/globals.css`:
  - Currently defines `:root` and `.dark` color variables (essentially the zinc theme and its dark variant).
  - Imports the theme CSS files: `zinc.css`, `catppuccin.css`, `t3chat.css`, `claude.css`.
  - After this phase, color-variable definitions will be migrated into TS theme objects and applied at runtime.
- `src/styles/themes/*.css` (`zinc.css`, `catppuccin.css`, `t3chat.css`, `claude.css`):
  - Each file defines color variables and sidebar-related variables for `data-theme='{name}'` and `data-theme='{name}' .dark`-style selectors.
  - These are reference sources for constructing equivalent TS theme objects (light+dark variants).

### Established Patterns

- Theme mode (`light` / `dark` / `system`) is controlled centrally by `SettingContext` via `classList` on `<html>` and `matchMedia('(prefers-color-scheme: dark)')`.
- Theme color selection is persisted through the settings system using `update_settings` Tauri command and `GeneralSettings.theme_color`.
- Tailwind styling is heavily reliant on CSS variables (`--background`, `--foreground`, etc.) wired via shadcn's `@theme` mapping in `globals.css`.

### Integration Points

- `SettingProvider`'s theme effect (`useEffect` in `src/contexts/SettingContext.tsx`:148-184):
  - Where the new theme-engine utility should be called to set per-theme variables after determining the effective mode and theme name.
- `AppearanceSection` (`src/components/setting/AppearanceSection.tsx`:75-137):
  - Where the preset list is rendered and where the multi-dot preview implementation will live.
- Theme definition module (to be added, likely under `src/constants/theme.ts` or a related file):
  - Central location for TS theme objects mapping `(themeName, mode) -> token -> color`.
- Removal or reduction of theme-related imports from `globals.css`:
  - Replace static theme CSS imports and concrete `:root`/`.dark` color assignments with a minimal or none baseline, deferring to JS injection.

</code_context>

<deferred>
## Deferred Ideas

- **User-defined/custom themes**:
  - Importing themes via URL/JSON or a "design your own" editor is explicitly out of scope.
  - If added later, they should build on the same TS theme object + runtime injection infrastructure.
- **Live preview on hover**:
  - Temporarily applying a theme while hovering over its swatch is postponed; current phase focuses on click-to-apply with smooth transition.
- **Theme grouping and metadata**:
  - Grouping themes into "official" vs "experimental" or showing extra metadata (e.g. contrast tags) is deferred.
- **Theme name i18n**:
  - Translating theme names (e.g. `zinc` → local-language color names) is not needed for this phase.

</deferred>

---

_Phase: 17-dynamic-theme-switching-with-shadcn-preset-import-and-enhanced-color-preview_
_Context gathered: 2026-03-08_
