# Native selection controls

## Pre-implementation failure model

This slice uses BoardUI source `ec5c5eb3e29c2441be63b9422cd54f411ef23466`.
The source file SHA256 receipts are in [assets/source-lock.json](assets/source-lock.json).
The source files are `components/base/switch/switch.tsx`,
`checkbox/checkbox.tsx`, `checkbox/checkbox-glyph.tsx`,
`slider/slider.tsx`, and `segmented-control/segmented-control.tsx`.

Failures to exercise before acceptance:

- Pointer and Space must update a caller-owned Switch or Checkbox value once.
- Disabled controls must block activation without applying opacity twice.
- Visible labels and hidden labels must preserve accessible names.
- Focus must remain visible on the painted indicator, including keyboard navigation.
- Checkbox mixed state must be exposed and activate to checked.
- Source track, thumb, inset chip, tick, gradient, shadow, and disabled opacity must
  remain distinct in both themes and at 1x and 2x.
- Reduced motion must reach the same final paint without animation.
- Slider drag and keyboard changes must stay clamped and respect Step.
- Segmented arrows must move focus; Enter/Space must commit selection without
  clearing an already selected item. Disabled items must be skipped.
- Application adapters must preserve Changed reporting and caller state.

## End-to-end protocol

Reuse the migration's existing canonical software-view scenarios and first-slice
replay. Add these control states to the official React reference before claiming
visual acceptance: off/on/disabled/focused, mixed Checkbox, minimum/mid/maximum
Slider, and selected/unselected Segmented items. Record source identity, theme,
DPI, values, focus, accessible roles/states, screenshots, and SHA256SUMS in the
same ignored artifact directory. Compare geometry and pixel evidence against
real pinned source. Keep the first failure and use one targeted retry before
investigating a repeated failure. These files do not claim native-platform or
user visual acceptance.

## Scope

The initial public APIs support caller-owned state. Switch supports source md
pill/rectangle. Checkbox supports source md/sm and a separate mixed-state entry.
Slider is a single horizontal thumb; range and RTL are outside this slice.
Segmented uses source solid/plain paint and native group navigation. It does not
change the public selection value on arrow navigation alone.

## Current motion boundary

Switch thumb and Checkbox tick use source duration/easing. Checkbox creates a
native keyframe sequence when its tick mounts, including an initially checked
control. The tick sequence uses a fixed widget-local key and does not accumulate
per-toggle cache entries. Segmented uses native matched geometry for its shared selected surface, with
the source 200ms ease transition. Record motion screenshots separately from
final-state comparisons.

## Sheet lifecycle failure and repair protocol

Before implementation, `all-view-initial` retained failing `sheet-motion` and
`sheet-exit-stability` receipts under `.build/boardui-migration/`. The live Sheet
was removed before exit completion and its `OnClosed` callback cleared table
selection immediately. Existing motion assertions also expected the retired
platform top-slide geometry. The new source baseline is the centered Settings
Modal: scale 0.85 to 1, opacity 0 to 1 and blur 4 to 0 over 300ms with
`cubic-bezier(0.32, 0.72, 0, 1)`. Its backdrop uses 300ms ease-out opacity.

Repair failure inventory:

- Closing through Done, Escape or backdrop must retain live content until the
  source exit completes, then call `OnClosed` exactly once.
- Exit reversal must preserve transient state and native focus scope.
- Centered width/height changes must lay out content during the transition.
- Visual scale changes must not change logical table widths or clear selection.
- Source fading and blur must be measured as source motion rather than compared
  against the retired opaque top-slide raster.
- Reduced motion must finish without a retained invisible modal.
- Native scope removal must restore the opener after completed dismissal.

Replay the retained Sheet scenarios into a new artifact directory. Keep Done,
Escape, backdrop, resize, reversal, state cleanup and post-dismissal input
assertions. Compare centered geometry and normalized logical widths against the
source scale baseline. Preserve the original failures; do not overwrite them.

## Typography extraction failure and correction

The source/computed audit saved its first failure as
`.build/boardui-migration/typography-red.json` before changing extraction.
It found 70 mismatching scheme/style records across the two schemes. For example,
`title-2-semibold` is 20/26/600 in the fixed source but the incomplete probe
inherited 16/24/400. Tailwind removes unused `@theme` variables, so referring to
those absent variables is not an exact-source extraction.

The reference capture now parses the fixed typography CSS with the existing
PostCSS dependency. Every style must supply its size, line height, letter spacing
and weight declarations. It injects those exact four declarations into an
invisible, noninteractive, absolutely positioned probe span. The real browser
still resolves rem units and computed font values. Missing/duplicate declarations
or invalid computed values fail extraction. The official application CSS and
visible reference components remain unchanged; used-style screenshots retain
their original visual baseline. Generated resources and source locks must be
rebuilt and replayed by the migration owner after this correction. This source
extraction audit is not native visual acceptance.
