# Native BoardUI for mygo

A reusable native Go package for the shared BoardUI Desktop presentation. It has no React, WebView, Tailwind runtime or Node runtime dependency. Production GUI, setup and Quick Panel use this package through application-only desktopkit compositions. The independent demo shares its complete view with the software E2E scenarios. It does not connect to UniClipboard services or the system clipboard.

The sole source reference is the [official MIT export](https://github.com/BoardUI/boardui/tree/ec5c5eb3e29c2441be63b9422cd54f411ef23466). Source hash, exact font/icon/dependency identities and notices live in [assets/source-lock.json](assets/source-lock.json). The fixed source's button press scale is 0.98; newer skill rules do not override it.

## Run

```sh
go build -o .build/boardui-demo ./app/desktop/cmd/boardui-demo
.build/boardui-demo --theme light --open
```

Use `--theme dark`, `--width 360` and `--height 720` for the other demo configuration. The demo supports client areas of at least 320×640 DIP; it keeps this bounded form fully reachable. Hosts with larger or dynamic bodies must supply their own scrolling body. Closing this independent demo has no product-service lifecycle side effects.

Run the canonical software E2E:

```sh
make desktop-e2e CASE=boardui-settings
make desktop-e2e CASE=boardui-states
```

Run the actual official React comparison and both canonical scenarios in one replay:

```sh
python3 app/desktop/boardui/e2e/replay.py --node /absolute/path/to/node
```

Node >=22.12, npm, ego-browser and Pillow are reference/acceptance tools only. Replay refuses existing output directories, retains first failures and prints its unique artifact directory. See the [native component contract](../../../docs/contracts/boardui-native-contract.md) and [implementation receipt](../../../docs/exec/boardui-mygo-implementation.md) for evidence and limitations.

## Public interface

Call `LoadFonts()` once before creating a view. `Kit{Dark: true}` selects immutable source-derived resources. Call `Kit.Apply(c)` at the beginning of the view. Read a button's `.Clicked()` result. Pass caller-owned value/open pointers to Input, Select and Dialog. Give repeated controls stable keys with `c.Key(id)`.

```go
kit := boardui.Kit{Dark: false}
kit.Apply(c)
kit.Input(c, &name, boardui.InputProps{
    Label: "Name", Description: "Your display name.",
})
if kit.Button(c, "Save changes", boardui.ButtonProps{}).Clicked() {
    // Validate and persist in the host.
}
```

Button variants are `primary`, `secondary`, `ghost`, `danger`. Sizes are `medium`, `small`, `xs`. Icon-only controls require a nonempty accessible Label. The bounded glyph vocabulary is `settings`, `close`, `check`, `eye`, `user`, `chevron-down`. Input sizes are `medium` and `small`. Select sizes are `md` and `sm`. Select typeahead is not implemented in this first slice; selection is read from its bound value. Unknown resource names and missing required state pointers are programmer errors; the package panics rather than silently drawing approximate fallback resources.

## Resource generation

The tracked `reference/` directory contains only the reference fixture, preparation/capture/generation scripts and exact npm lock. Preparation fetches the official fixed export into ignored `.build/boardui-reference/` and verifies every file against the research manifest. It never installs into production UI. The real Vite/Tailwind pipeline resolves the original CSS. Browser computed styles and sRGB Canvas conversion supply numeric resources; the generator does not manually resolve CSS aliases or guess Tailwind pixels.

After preparing and starting the isolated reference server, capture resources with `reference/capture.py --resources --only-resources --out <new-directory>`, then run `node reference/generate.mjs` from the repository root. `generate.mjs` uses the private build's exact dependency graph. Review the resulting token/source-lock diff and replay before accepting an update. Never change only generated constants.

The complete MIT/OFL/Remix notices are embedded and retained under `assets/`. Inter is registered once per process. Six parsed SVG glyphs are cached once. Neither cache is keyed by widget IDs. Pro source and the 18 newer source-unavailable free entries are excluded.

## Production integration and selection controls

The removed three-platform UI Kit has no import alias or skin fallback. The
internal `desktopkit` package retains application-owned layouts, table/list
selection, paging and task flows; it delegates public controls and theme data
to this package. It does not define another theme palette.

Switch supports the inspected md pill/rectangle. Checkbox supports md/sm and
explicit mixed state. Slider supports one horizontal thumb. SegmentedControl
supports solid/plain single selection; arrows move focus, Enter/Space select.
DressButton and DressLinkButton let maintained native action bases share the
source paint. Hidden visual labels preserve the accessible names of controls
inside application settings rows. See [selection controls](CONTROLS.md) for the
precise source/state and acceptance boundary. Source-backed implementation is
not a claim that every state has paired React raster acceptance.

Replay the production migration with its canonical companion cases:

```sh
python3 app/desktop/e2e/replay_boardui_migration.py --out .build/boardui-production-review
```

The artifact directory must be new. Native windows, OS input methods and
platform accessibility require separate platform evidence. Windows/Linux cross
compilation is separate from execution on those operating systems.

## Code-defined color themes

The [color catalogue](COLOR-TOKENS.md) extracts the complete current palette for
both schemes. Generated source resources remain unchanged. `Token`, `Gradient`
and `Shadows` are source-only lookups. Use `Kit.Color`, `Kit.Gradient` and
`Kit.Shadows` when rendering a theme.

Define up to **12 base colors per scheme** with `Palette`. For a brand-only
change, supply only `Accent`. The constructor derives the accent ramp, primary
button gradients, focus rings, selected controls, switch chips, selection,
information notices and accent shadows. No token names are required.

```go
colors := boardui.NewPaletteTheme(
    boardui.Palette{Accent: ui.RGB(168, 85, 247)},
    boardui.Palette{Accent: ui.RGB(192, 132, 252)},
)
// On the view thread, select the host's resolved light/dark scheme.
kit := boardui.Kit{Dark: dark, Colors: colors}
kit.Apply(c)
kit.Button(c, "Save", boardui.ButtonProps{})
// Raw mygo controls read the same ui.Theme from c.
ui.Button(c, "Native action")
```

| Base field | Meaning |
|---|---|
| `Background` | Page background |
| `Surface` | Control background |
| `Elevated` | Raised surface background |
| `Text` | Primary text |
| `TextMuted` | Secondary text |
| `Border` | Default border |
| `Accent` | Brand and selection color |
| `AccentText` | Text on the accent; automatic black/white choice when omitted for a new accent |
| `Success` | Success state |
| `Warning` | Warning state |
| `Danger` | Error and destructive state |
| `Overlay` | Dialog mask and shadow hue |

Construct once outside the view. `ui.Color{}` means inherit that scheme's base
value. Empty light/dark palettes preserve every source token exactly. Light and
dark are independent; a light-only change never modifies dark. `DefaultPalette`
returns a value copy for inspection or editing. A nil theme and a zero-value
`ColorTheme` retain the source presentation. There is no mutable global theme.

Neutral hover, pressed, disabled, border and placeholder states mix the base
colors through mygo's existing Oklab API. Success, warning and error tints use the
surface and their state color. Shadow geometry and per-layer opacity stay fixed;
`Overlay` supplies their hue. Named categorical tag hues retain their meaning;
their background tint adapts to the new surface. They do not become brand hues.
Literal `text-white` remains white. `text-on-accent` and `text-on-danger` carry
foreground roles separately. Automatic black/white selection uses sRGB relative
luminance on the base fill; it is not full-theme contrast certification.

Use exact final token overrides only for special adjustments:

```go
colors, err := colors.WithOverrides(
    map[string]ui.Color{
        "border-focus-ring": ui.RGB(0, 180, 150),
        "dialog-backdrop": {}, // Exactly transparent, not an omitted base.
    },
    nil,
)
if err != nil {
    return err
}
```

`WithOverrides` validates names and copies the resolved palette. Overrides take
precedence and do not re-derive other colors. Input maps and `Tokens(dark)`
snapshots never alias theme state. Unknown names return an error.
`NewColorTheme(lightTokens, darkTokens)` remains the lower-level token constructor.
Its primary gradients, selected-checkbox shadow and native accent/selection/
scrollbar defaults follow the corresponding token overrides. Other semantic
tokens remain independent at that level. Set base colors through `Palette` to
obtain complete state derivation. Gradient endpoints use
`gradient-<name>-from` / `gradient-<name>-to`; shadow colors use
`shadow-<name>-<zero-based-layer>`.

Application compositions accept `desktopkit.Kit{Dark: dark, Colors: colors}`.
The main Desktop exposes `SetColorTheme(colors)`. QuickPreview exposes the same
setter for its panel and satellite. Setup accepts `Model.Colors` or
`Screen.SetColorTheme(colors)`. Independent GUI, Quick Panel and setup hosts must
supply the same theme in each process. Setters run on the view thread; invalidate
the host window after changing the main/setup theme. These operations retain
caller-owned editors, selections and pending work. They do not persist theme
preferences or replace native OS decoration and menus.

Run the shared color-theme operation chain with:

```sh
make desktop-e2e CASE=boardui-palette
make desktop-e2e CASE=boardui-theme
```

The software-view report retains PNGs, accessibility checkpoints, resolved
`palette-tokens.json` / `theme-tokens.json`, source hashes and assertion results. Native windows,
compositor output and user visual approval require separate evidence.
