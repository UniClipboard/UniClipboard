# Native BoardUI for mygo

A reusable native Go package for the first BoardUI settings slice. It has no React, WebView, Tailwind runtime or Node runtime dependency. The independent demo shares its complete view with the software E2E scenarios. It does not connect to UniClipboard services or the system clipboard.

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
