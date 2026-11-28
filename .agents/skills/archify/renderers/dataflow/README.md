# Data Flow Renderer

Render `diagram_type: "dataflow"` JSON files into the standard Archify HTML
template.

```bash
node archify/renderers/dataflow/render-dataflow.mjs input.dataflow.json output.html
```

The renderer validates input against `archify/schemas/dataflow.schema.json`
with the bundled standalone validator. No dependency installation is required.

If `output.html` is omitted, the renderer uses the required `meta.output` value
from the JSON file.

## Input

Data-flow JSON files must set:

```json
{
  "schema_version": 1,
  "diagram_type": "dataflow",
  "meta": {
    "title": "Product Analytics Data Flow",
    "output": "product-analytics-dataflow.html",
    "viewBox": [940, 720]
  },
  "stages": [],
  "nodes": [],
  "flows": [],
  "cards": []
}
```

A complete worked example lives at
`archify/examples/product-analytics.dataflow.json`.

The schema lives at:

```text
archify/schemas/dataflow.schema.json
```

## Legend

The default visual legend derives kinds from `flows[].variant` (omitting
`variant` means `default`) and adds `database` only when a database node exists.
Supported `meta.legend.entries` keys, in stable order, are `emphasis`,
`security`, `dashed`, `database`, and `default`. Flow variants remain
visual-only because Archify has no compiled edge-kind facts in this slice. A
present `database` entry is different: it comes from exact
`nodes[].type: "database"` facts, so it publishes the normal Semantic Legend
count, accessible name, and keyboard interaction. Forcing `database` visible
without a database node keeps it visual-only.

## Layout budget

| Constant | Value |
|----------|-------|
| viewBox | omitted width fits all stage frames and nodes, including authored widths, with 24px right padding and a 480px minimum (five default-width stages need 1068); height defaults to 720 and fits content in fully automatic showcase; schema minimum `[360, 360]` |
| Stages (2–5) | centers at x = 100 + stage×215; stage band 168 wide, header at y 46 |
| Row tops (`row` 0–4) | y = 128, 242, 356, 470, 584 (plus `yOffset`) |
| Default node | 112×58 |
| Node area | x within `[24, width − 24]`; y within `[104, height − 74 − node height]` |
| Node spacing | ≥10px between any two nodes (checked across stages and rows) |
| Flow length | ≥34px between endpoints |
| Legend row | y = height − 36 |

Route presets for flows: `straight`, `vertical-channel`, `bottom-channel`,
`top-channel`, explicit `via` points, or the default `auto` (midpoint elbow).

An authored `meta.viewBox` remains authoritative. When it is omitted, canvas
width fits stage frames and nodes in both `standard` and `showcase`, including
explicit node widths. Enlarging width cannot repair left-edge overflow.

Showcase node sizing, typography, automatic height, port bridges and label
placement apply only when `meta.viewBox` and every node's `width` are omitted.
An authored canvas or node width preserves those established layout behaviors,
as does `standard`. The effective quality profile is
`process.env.ARCHIFY_QUALITY_PROFILE || meta.quality_profile`; the public CLI's
`--quality standard|showcase` sets the environment override.

For a fully automatic showcase, node widths can grow within their stage and
canvas budget to keep supporting text readable at desktop projection. Height
follows actual node bounds, routed points and rendered label plates, leaving
24px below content plus the 74px stage/legend reserve. Rows and authored routes
remain authoritative; inactive channel controls do not enlarge the canvas.

Crowded incoming arrows can expand a default node vertically within its row's
spare space, up to 104px high. Its row centre and text group stay in place, so
clear same-row outputs remain straight. The 16px corner gutters and 10px node
gaps remain reserved. An explicit node height or `yOffset`, or an incident
route, side or label pin, preserves the original box. Space limits may
still leave very dense inputs crowded; no relationship or label is removed.

For facing automatic ports less than 16px apart across the route axis, an
unshared endpoint can align with the spread endpoint when the straight path
is clear. This preserves the shared slots, the 16px corner gutter, occupied
ports and authored route/label controls. When both ends are spread or the
direct path is blocked, the existing perpendicular bridge remains.
When obstacle routing reverses a branch's initial travel direction, a shared
side can reorder its existing slots by the settled routes' first turns. This
tries at most 16 fully automatic groups of two to eight ports before placing
labels. A group is accepted only if a route becomes straight and no member
gains length or bends, adds crossings, folds back, or loses node, border or
corridor clearance. Explicit side, route and label controls keep their slots.

After routes and labels settle, a bounded pass can retry this alignment while
keeping every other route and label fixed. It checks at most 16 label placements;
the straight path must retain port spacing and a clear, nearby label position.
Grid repairs also retain the route rhythm floor before replacing a valid bridge.
Unpinned labels stay within 36px mask clearance of their own route. If a
label remains inside a node because a foreign route occupies its nearby slot,
a bounded joint repair can shift that route and place the label again. It
checks at most 16 label slots and 16 route candidates without adding bends
or crossings. Unresolved placements keep their collision diagnostic.
Explicit `labelAt`, `labelDx`, `labelDy` and `labelSegment` remain authoritative,
including zero offsets. Existing valid labels are retained.

If a complete automatic label still collides after routing, its text can wrap
to two rows beside a horizontal segment. The font size, wording, classification
and all routes stay unchanged. The plate stays within that segment's span and
at least 4px from every route, rather than moving above a nearby node. This
last capacity repair tries at most eight word-boundary splits and 64 positions
per blocked label. If neither row fits, the original diagnostic remains.

## Design Rules

- Use stages for data lifecycle boundaries: source, ingest, process, store,
  consume.
- Place nodes by stage index and row index; do not hand-place raw SVG for the
  common case.
- Use flow labels to name the data asset, not the transport primitive:
  `clickstream`, `identity map`, `normalized facts`, `feature vectors`.
- Use `classification` for short sensitivity or governance context:
  `PII touch`, `non-PII`, `approved only`, `batch`, `read-only`.
- Use `security` for PII, policy, consent, access-control, or restricted joins.
- Use `emphasis` for the primary data path and `dashed` for async or batch
  derivations.
- Keep labels short enough to fit in narrow previews.

Schema violations exit non-zero with path-prefixed messages annotated with the
element's id or label. The renderer additionally fails when it can detect
layout problems, including missing stages, duplicate node IDs, nodes outside
the readable diagram area (`dataflow/node-above-area` / `dataflow/node-below-area`;
the below-area diagnostic offers `meta.viewBox[1]` raises only while the raised
page still fits the target viewport, and names the height range that does —
page containment is the modeled axis, while projected text readability is
reported separately by the delivery checks (`viewer/projected-text-readability`)),
node overlap, labels colliding with nodes or other
labels, labels wider than their node, unknown flow endpoints, missing flow
labels, unreadably short flows, flows crossing unrelated nodes (2px Clean Flow
clearance), or stages that exceed the viewBox. Stage frames remain intentional
pass-through containers. Text width
is estimated CJK-aware: fullwidth glyphs count as two units.

Set `meta.quality_profile` to `showcase` for polished delivery. Unresolved proper
X crossings fail with `composition/proper-crossing`; default `standard`
keeps them as artifact-receipt warnings. In automatic showcase layouts without
an authored canvas or node widths, the renderer compares clear detours with
visibly separated crossovers. It prefers a short clear route, but may retain
a shorter automatic crossing with an opaque halo instead of a long perimeter
detour. Before comparing crossings, bounded route searches try to repair
automatic paths that enter unrelated nodes or share ambiguous corridors.
They preserve node clearance and corridor separation; failed searches still
leave the obstruction diagnostic in place. Both crossing paths need matching
halos; the artifact check verifies
their geometry and reports `resolvedCrossovers` for visual review. Supplied
`via`, non-auto `route`, and channel coordinates retain their geometry and
crossing diagnostics. Collinear stage corridors are outside
the proper-X rule, but a separate gate warns in `standard` and fails in
`showcase` when unrelated flows overlap for at least 8px. Shared semantic
endpoints, point touches, and shorter overlaps remain valid. Showcase also
rejects any route segment below 8px and any interior turn segment below 16px;
ordinary 8–15px endpoint stubs remain valid.
