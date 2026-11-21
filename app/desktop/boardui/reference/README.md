# Official React reference and generated native resources

This tooling is not a production runtime dependency. It materializes the real official MIT BoardUI export at `ec5c5eb3e29c2441be63b9422cd54f411ef23466` under the ignored `.build/boardui-reference/upstream/` directory. The production package embeds only the necessary generated resources, Inter font and six SVG glyphs.

The fixture imports the unchanged official Button, Input and Select. Its bounded dialog composition uses the actual settings-modal surface/motion recipe, not an invented “official generic Modal”. React Aria supplies dialog focus management. The fixed settings-modal implementation's global Escape handling and missing explicit focus restoration/trap are not silently attributed to React Aria. The native API contract specifies the stronger interaction needed by the standalone settings form.

## Replay

Run from the repository root with Node 22.12+ selected:

```sh
python3 app/desktop/boardui/reference/prepare.py
```

Read `.build/boardui-reference/node-path.txt`. From `.build/boardui-reference/`, run that exact Node executable with `node_modules/vite/bin/vite.js --host 127.0.0.1 --port 4177 --strictPort`. This localhost server serves the isolated fixture only.

```sh
python3 app/desktop/boardui/reference/capture.py --out "$PWD/.build/boardui-reference/acceptance"
```

The capture tool uses the documented ego-browser API. It opens its own TaskSpace. It captures full PNG checkpoints and per-checkpoint `-geometry.json`, then closes its exact TaskSpace in a separate final ego-browser script. It retains capture and cleanup logs plus cleanup exit status. A failed capture retains its TaskSpace for diagnosis; a failed cleanup fails the command. Capture uses direct CDP `Page.captureScreenshot` with `fromSurface=true`, `clip.scale=1` and no image resizing. Every PNG IHDR must equal viewport × DPR: 800×760 at 1x and 1600×1520 at 2x. The higher-level screenshot helper defaults to CSS-pixel output, so it is not used for this device-pixel comparison. It does not inspect personal pages or use the clipboard. It fails immediately on a required action/assertion/capture error; the output directory remains intact. Do not retry into the same output directory after a failure.

For resource regeneration, with the same reference server running:

```sh
python3 app/desktop/boardui/reference/capture.py --out "$PWD/.build/boardui-reference/resource-artifacts" --only-resources
node app/desktop/boardui/reference/generate.mjs
```

Use the exact Node from `node-path.txt` for the second command. Generation reads real browser computed CSS and TypeScript/CSS source declarations. It emits `tokens_gen.go` and `assets/source-lock.json`. Do not edit generated values by hand.

## Evidence and interpretation

The resource generator records 74 source semantic/accent colors per light/dark scheme, 52 complete type styles, seven control-size geometries, ordered shadows, two-stop gradients and a finite six-glyph set. Color conversion uses the reference browser's Canvas2D sRGB eight-bit output. Native output compares in sRGB; wide-gamut output is not claimed.

Inter is the variable TTF from Inter 4.1. Both fixtures use the same bytes under the `BoardUI Inter` family. The original site's `next/font/google` download is replaced only by this explicitly frozen font fixture. The optical-size axis and OS rasterization still need measured comparison. CJK/emoji use the actual platform fallback and do not prove identical cross-OS text shaping.

The native and reference fixture use shared initial values: Name `Maya Chen`, password `secret`, read-only `Managed by workspace`, disabled `Not available`, Role `member` (`Member`, `Administrator`, `Viewer`). The default viewport is 800×760 CSS px/DIP, panel width 480, padding 24, section gap 24, field gap 16, footer gap 12. This layout is a bounded test composition, not a claim that the original 871×614 full settings application has been ported.

The source has an active button scale of 0.98, 220ms press and 420ms release. Its backdrop class is black at 70%, even though an older source comment says 20%. Actual fixed source wins over comments and newer skill recipes.

`source-lock.json` binds official source, font, icon module/glyphs, dependency lock, compiler/browser identity and scalar origins. The package retains the BoardUI MIT notice, Inter OFL and Remix Icon License separately. Remix glyphs are functional minor assets in this component package; no standalone icon library is offered.

## Device-pixel capture correction

The first reference run used the screenshot helper and produced 800×760 PNGs even at DPR 2. That run is retained as a rejected capture identity. A `raw:true` probe also returned CSS-sized output and is retained as a failure. Direct CDP capture then produced original 1600×1520 PNGs at DPR 2. The corrected 13-checkpoint run passed the dimension assertions and interaction assertions. No existing screenshot was resized.

Geometry compares painted surfaces consistently: listbox `bounds` is the popup surface, and `rawListboxBounds` retains the inner listbox content rectangle. Option rectangles remain unchanged. The reference interaction chain asserts that one Cancel click outside an open listbox also closes the containing settings dialog. It reopens the fixture and restores the Administrator selection before the invalid-state screenshot. This is a host composition scenario, not a claim that the fixed source contains a generic BoardUI Dialog.

Select timing is read from the real browser computed styles. The pinned Tailwind build resolves the popover to `cubic-bezier(0, 0, 0.2, 1)` over 150ms and the chevron to `cubic-bezier(0.4, 0, 0.2, 1)` over 200ms. The generator emits all four curve coordinates plus duration. Unknown timing or duration syntax fails extraction; native polynomial easing helpers are not treated as equivalent CSS curves.
