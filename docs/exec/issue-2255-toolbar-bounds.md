# Issue 2255: macOS history toolbar bounds

## Scope and result

The shared UI Kit toolbar keeps a fixed 52-point band, balanced 10-point vertical padding. The toolbar and history content form a continuous surface without a horizontal separator, as requested during visual review. Search expands to at most 460 points and shrinks with its input to the available width. History actions, selection and filters retain their existing composition. Light toolbar capsules use a continuous one-point inset outline with the existing separator token. The outline consumes no layout space. The floating detail action bar and its Glass send button share the light outline. Dark rendering and other glass surfaces are unchanged. Native titlebar configuration and drag regions are unchanged.

The source baseline is `bd89ff5a12885079e4259c3a72c90ab5e8b420ca`. Acceptance runs use a dirty frozen snapshot. Each report manifest binds the full source file hashes, executable hashes, toolchain and host OS. Each `SHA256SUMS` binds the retained report, geometry JSON, PNGs and continuous recording. The source and architecture verification marker are committed separately so the marker names the verified source commit.

## Reproduction

The original `toolbar-alignment` case passed because it checked menu centers only. The expanded full-composition case failed before the fix. A 52-point toolbar placed its 32-point search and layout controls at Y=14, leaving 14 points above and only 6 below. Expanded search in a 600-point window occupied X=228 through X=688.

The baseline matrix report is `.build/desktop-e2e/20261116T034222Z-ef659043/index.html`. It contains 128 failed geometry checkpoints. The original native `history-search-copy` baseline opened and recorded production content but failed waiting for Cmd+F. That separate baseline result is `.build/desktop-e2e/20261116T034135Z-fc38be2d/index.html`.

## Repeatable acceptance

Run from the repository root:

```sh
make desktop-e2e CASE=toolbar-alignment
make desktop-e2e CASE=search-disclosure
make desktop-e2e CASE=native-menus
make desktop-e2e CASE=toolbar-bounds-light-600
make desktop-e2e CASE=toolbar-bounds-dark-600
make desktop-e2e CASE=toolbar-bounds-light-1100
make desktop-e2e CASE=toolbar-bounds-dark-1100
```

The software matrix exercises 32 scenarios: 600, 800, 1100 and 1440-point widths; light/dark; list/grid; collapsed/expanded search. The 600 and 1100-point scenarios use 2x rendering; the other widths use 1x. Assertions check toolbar height, vertical insets, containment, menu alignment and search-input hit-target containment. The initial geometry-only report is `.build/desktop-e2e/20261116T035005Z-7d936a35/index.html`. The user rejected its light-theme appearance. The current passing report, including pixel contrast, is `.build/desktop-e2e/20261116T041026Z-f0c7bac9/index.html`.

Existing search disclosure and menu scenarios passed in `.build/desktop-e2e/20261116T034908Z-9c56d74d/index.html` and `.build/desktop-e2e/20261116T035057Z-77ba7b13/index.html`. These are software-view evidence. Sidebar collapse also passed in `.build/desktop-e2e/20261116T035446Z-df90e303/index.html`.

Native cases use the production Desktop, service, Engine, SQLite and IPC with private profiles and synthetic history. They assert actual window width and control bounds, type a search, switch to grid and open the command menu. The E2E-only hook applies requested dimensions to initial and post-startup sizing. Production dimensions remain 1100x720.

Native host: macOS 27.0, Apple Silicon. Captures use 2x rendering. Each screenshot has a JSON sidecar with logical dimensions, pixel dimensions, theme and capture scale. This host does not reproduce the reported macOS 15.6 environment.

| Native case | Report directory under `.build/desktop-e2e/` | Result |
|---|---|---|
| light, 600x480 | `20261116T041027Z-b689d229` | PASS |
| dark, 600x480 | `20261116T035227Z-dc2a57bd` | PASS |
| light, 1100x720 | `20261116T043438Z-71bccd5b` | PASS |
| dark, 1100x720 | `20261116T043533Z-c018cbd0` | PASS |

## Light-theme visual follow-up

The user accepted dark screenshots and rejected the initial light screenshots. The geometry fix alone left the glass's directional top/bottom highlight nearly white against white. A toolbar-only material helper adds a continuous one-point inset outline with the existing separator token. It applies to the layout switch, search and command capsule. The user later extended the same treatment to the floating detail action bar and its Glass send button. It does not change layout, hit targets, the primary Copy button, dark material or other glass surfaces.

The full-composition pixel check follows each capsule's rounded contour and samples the background beyond the focus halo. At least 80% of the central contour samples on each top/bottom edge must differ from the background by an average of 12 RGB levels. Replaying the rejected screenshots fails 80 of 96 light-edge checks. The current matrix passes every edge, with minimum visible fraction 0.9667. The initial pixel-regression report is `.build/desktop-e2e/20261116T040805Z-a73952d1/index.html`.

All 16 current dark software checkpoints are byte-identical to the previously accepted dark checkpoints. Both new native light reports passed; their source, binary and artifact checksums were verified. Four current light screenshots (1100/600 points, search collapsed/expanded) were sent through Lody. Liquid-glass hover, press and menu regression passed in `.build/desktop-e2e/20261116T041232Z-f3480e79/index.html`. User visual acceptance of this correction is pending.

## Detail action bar extension

The user also requested the same light outline for the floating detail action bar and its Glass send button. Before implementation, the full-composition `liquid-glass` case failed all four top/bottom edge checks, with 0/5 visible samples per edge. That report is `.build/desktop-e2e/20261116T043013Z-9b9d470f/index.html`.

Both surfaces now reuse the existing light-outline helper, renamed `outlinedGlassSurface`. The primary Copy button, layout and hit targets are unchanged. The current `liquid-glass` report is `.build/desktop-e2e/20261116T043115Z-88efca25/index.html`: all four edges pass, hover resets, and the secondary action enters menu tracking. All three dark checkpoints are byte-identical to the prior report. Scoped lint and formatting checks pass.

The current native light 1100-point report is `.build/desktop-e2e/20261116T043116Z-58d056ae/index.html`. It includes the updated action bar. Its artifact checksums were verified and its collapsed-search screenshot was sent through Lody. Native compact mode does not show this detail action bar until detail navigation. User visual acceptance remains pending.

## Continuous toolbar and history surface

The user rejected the horizontal separator because the toolbar and history should read as one surface. The final macOS toolbar retains fixed height, balanced padding and capsule outlines, with no horizontal separator. Updated native light and dark 1100-point runs passed in `20261116T043438Z-71bccd5b` and `20261116T043533Z-c018cbd0`. The new light screenshot was sent through Lody.

## Evidence limits

Native captures record window content. They exclude OS decorations. Traffic-light interaction, physical titlebar dragging, tooltip hover, OS focus delivery, macOS 15.6 and native 1x displays remain NOT_RUN. Native menus and keyboard input in these cases use app-scoped automation. No system clipboard or real profile is used.

## Checks

The production Desktop build, scoped golangci-lint v2.15.0, goimports and whitespace checks passed. `go run ./tools/docscheck` passed with eight stale-page warnings. The architecture verification marker is updated after the scoped source commit with `go run ./tools/docscheck -bump docs/architecture/overview.md`.
