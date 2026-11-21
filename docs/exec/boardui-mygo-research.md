# BoardUI to native mygo: research and migration proposal

Status: research complete within the evidence available below; implementation, live-registry reconciliation, visual comparison and native acceptance are NOT RUN. This document proposes future work. It does not change the current architecture, public contracts or production UI Kit.

Research date: 2026-11-20. Repository: `https://github.com/UniClipboard/UniClipboard.git`. Branch: `research/boardui-mygo`. Independent worktree: `/Users/mark/.lody/repos/github---uniclipboard---uniclipboard/worktrees/5aef43a7-6c77-47a0-b192-5fdea22d4c7f`. Baseline after fetching `origin/v2`: `ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0` (HEAD and fetched ref matched).

## 1. Recommendation and decision boundary

Build a native, reusable BoardUI component package on the pinned mygo behavior primitives. Preserve BoardUI's own visual language on every OS. Start with one isolated settings-form slice. Keep the existing product UI and three-skin Kit intact until a separate adoption decision.

Use the MIT public export as the first immutable reference. Do not describe it as today's live registry. Do not include Pro implementations in a public native library without an explicit additional grant. A normal Pro purchase is not sufficient evidence for that grant.

A pure fourth skin of the current Kit is insufficient for this goal. Its API and layouts encode different geometry, state and native-menu decisions. Reuse its generation location, lifecycle conventions, existing accessibility practices and host integration. Reuse mygo behavior directly when the Kit's visual composition differs. Do not introduce a universal renderer abstraction, WebView wrapper, Tailwind runtime, Node process or third production Kit.

This recommendation leaves two explicit decisions for the user before implementation: approve a free-only first slice against the pinned export; choose whether the eventual library is public/reusable or private/product-internal. Pro scope and permission require a later decision supported by the rights holder's answer.

## 2. Primary evidence and exact versions

| Evidence | Verified identity | Meaning and limitation |
| --- | --- | --- |
| [Official public repository](https://github.com/BoardUI/boardui/tree/ec5c5eb3e29c2441be63b9422cd54f411ef23466) | `ec5c5eb3e29c2441be63b9422cd54f411ef23466`, commit time `2026-10-03T00:59:08+03:00`; subject `Export from boardui bf69775` | Actual React source, not a reconstruction. `bf69775` is an export label, not a complete independently verified upstream commit. |
| [Public package manifest](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/package.json) | private starter `0.1.0`; React/DOM `19.2.4`, Next `16.3.3`; Tailwind v4 | Not a BoardUI component-library release. Most dependency ranges float; no committed lockfile in this export. |
| [CLI npm metadata](https://registry.npmjs.org/boardui/0.5.9) | CLI `0.5.9`, published `2026-10-31T18:02:06.841Z`; tarball integrity and SHA256 in manifest | The CLI is versioned independently from components. It identifies official `/r` and authenticated `/api/pro/r` endpoints. |
| [Public registry](https://www.boardui.com/r/registry.json) | HTTP 402, both direct retrieval and actual `boardui list` | No successful live index or item hash. Do not infer an account/paywall cause from this HTTP status. |
| BoardUI skill and four references | version `2026.11.10` | Read as discovery guidance. Extra catalogue items are not verified source. |
| mygo dependency | `v0.8.4-0.20261108082913-9233663736ca` | Source/API audit used this exact module-cache version, not latest upstream. |

The public source verifies the requested identity: BoardUI from `boardui.com`, React, Tailwind CSS v4, semantic CSS tokens, React Aria controls. npm's older repository URL names `mertcanesmergul/boardui`; the public export lives under `BoardUI/boardui`. These are recorded separately.

The [source manifest](boardui-mygo/sources.json) contains hashes for every file in the public export, CLI tarball integrity, observed HTTP failures and dependency identity. The [item matrix](boardui-mygo/matrix.md) provides readable coverage. The [machine catalogue](boardui-mygo/catalogue.json) provides exact source paths, per-item hashes, direct imports, literal domains, explicit boolean/literal props, direct CSS states and per-item gap assessments.

### Catalogue reconciliation

| Population | Count | Interpretation |
| --- | ---: | --- |
| Pinned public README install items | 69 | 38 base controls, 17 application blocks, 14 foundations/utilities. All 69 map to real public files. |
| Additional free entries in newer skill | 18 | Modal, drawer, popover, combobox, toast, accordion, spinner, progress, meter, skeleton, command-palette, context-menu, search-field, number-field, time-field, tag-group, date-calendar, color-picker. Source unavailable in pinned export. |
| Pro component descriptions | 30 | Public names/descriptions only; no restricted source fetched. |
| Pro template descriptions | 10 | Whole routes and component subtrees; separate from primitives. |
| Evidence union | 127 | 69 + 18 + 30 + 10; not a live component total. |
| Public export component TSX files | 90 | Includes helpers, previews and app composition; not 90 installable controls. |

The indexed [official home page](https://www.boardui.com/) advertises 104 unique components and 10 templates. That number cannot be reconciled to the inaccessible live registry from current evidence. The skill has 116 headings, including foundations and templates. Do not equate headings, install items, exported functions, chart cards or visual permutations. `agent-log` and several foundations appear in the public export but not the newer skill catalogue. Preserve both identities instead of deleting apparent discrepancies.

Literal extraction is mechanical TypeScript AST inspection using TypeScript from the isolated CLI installation. It includes non-exported aliases such as ButtonSize. It does not expand inherited React Aria or HTML attributes. A source file without a variant enum is not evidence of one state only. Catalogue domains are exact for inspected local declarations; full supported state combinations still require the E2E expansion below. The matrix does not invent Pro enums or missing-source sizes. Shared helper domains in a row are labeled as helper data in JSON; for example a settings dialog can inherit storage-file enum domains without those becoming visual dialog variants.

## 3. License and asset boundary

The fixed public repository's [LICENSE](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/LICENSE) grants MIT rights. A derivative Go port of that software may be modified, distributed and used commercially with the copyright and permission notice retained. This conclusion comes from the license file, not free downloadability or the CLI's MIT label.

The indexed [BoardUI License](https://www.boardui.com/license), updated 2026-10-31, expressly places Free source under MIT. Its Pro rules permit product use and modification, but restrict recognizable derivative source redistribution and competing component libraries/design systems. A private internal design system is distinguished from an offered library. A public Go port including Pro therefore needs written permission. The page also contains a broad statement about installed source; preserve the fixed MIT evidence and resolve any scope ambiguity before using a newer source snapshot.

The [terms](https://www.boardui.com/terms), updated 2026-08-21, prohibit reaching Pro source without a valid license. No Pro endpoint, login, key, purchase or private account page was used. Search-index text supplied the license/terms evidence because direct HTTP returned 402. Re-read the actual pages before procurement or external publication; no current Pro entitlement was established.

| Asset | Primary reference | Migration handling |
| --- | --- | --- |
| Inter | [OFL 1.1](https://github.com/rsms/inter/blob/master/LICENSE.txt) | Bundle exact approved font files with OFL/copyright. Do not sell font files alone. Modified fonts must respect reserved-name rules. Font binaries and hashes remain a first-slice prerequisite. |
| Remix Icon | [Remix Icon License v1.0](https://github.com/Remix-Design/RemixIcon/blob/master/License) | UI-kit inclusion is permitted where icons are a minor functional asset. Standalone/competing icon libraries and brand-identity use are restricted. Keep its license separate from MIT. Verify the license of the exact selected `@remixicon/react` package before extracting glyphs. |
| Social/brand artwork | Exported social providers and color logos | Do not infer trademark rights from source license. Use only for corresponding providers; review each retained asset. No need for all 24 in the first slice. |
| BoardUI name/logo, photos, placeholder art | Public source and product mark | Attribution does not grant endorsement or trademark rights. Use neutral synthetic fixtures; omit BoardUI product branding from the native library identity. |
| React Aria / Motion / Tailwind / Recharts | Upstream package notices | React Aria is Apache 2.0; the others are MIT per BoardUI's license page. Record exact installed dependencies/notices for reference fixtures. A Go reimplementation does not automatically need these JS runtimes. |

Future source ingestion must carry a source-ID-to-license map. Retain BoardUI MIT notice with substantial derived code/tokens. Retain exact third-party notices when bundling assets. Public metadata in this research is not permission to copy Pro source. No license-key data is included.

## 4. Design-system mapping

Authoritative files are `styles/theme.css`, `styles/typography.css`, `styles/globals.css`, `app/layout.tsx`, component source and their imports at the pinned commit. The theme has `@theme` primitive overrides, `:root` light semantics, `.dark` overrides and `@theme inline` semantic exports. Most raw palette values come from Tailwind defaults, so extracting only BoardUI's CSS loses required values.

### Colors, geometry and type

Preserve named semantic families: text, background, border, foreground/icon, button, separator and chart. Preserve the accent 50–950 ramp separately from content/chart blue. Default accent aliases blue; `blue-400` is overridden to `#3392ff`. Other explicit primitive overrides include slate-200 `#ebeff5`, neutral-100 `#f7f7f7`, neutral-200 `#ebebeb`, neutral-925 `#121212`. Resolve references for each scheme. Do not map all roles onto the current Kit's `primary`, `secondary` and `muted`.

Compile colors to typed resources with an explicit color-space policy. Resolve OKLCH and `color-mix` under the same CSS semantics as the React reference; document clipping/conversion to the native renderer. Keep gradient stop colors and alpha separate. Compare flat fills in an agreed sRGB capture profile. Never silently treat an unparsed color as transparent, as a generic permissive token reader could do.

At a 16px root, the exported type families are:

| Family | Size / line-height in CSS px | Weight-specific detail |
| --- | --- | --- |
| Large title | 64 / 80 | All four weights |
| Display 1–4 | 56/72, 48/64, 40/54, 32/44 | All four weights |
| Title 1–2 | 24/34, 20/26 | All four weights |
| Title 3 | 18/24 regular; 18/26 others | Do not flatten the regular exception |
| Headline | 16/22 | All four weights |
| Body / Body 2 | 14/20; 13/18 | All four weights |
| Caption 1 / Caption 2 | 12/16; 11/15 | Tracking 0.15px / 0.2px |

Weights are 400/500/600/700. Other listed tracking is zero. These are 52 composite styles, not 52 components. Use mygo `.Font`, `.FontSize`, `.FontWeight`, `.FixedLineHeight`, `.LetterSpacing` and appropriate font features. `.LineHeight` is a multiplier; passing CSS pixels to it is wrong. `FontFeatures("tnum")` supports tabular metrics where the reference requests them.

The React layout loads Inter with `opsz` and JetBrains Mono through `next/font/google`. Its sans fallback list is system sans plus Apple/Segoe/Noto emoji; Latin Inter does not supply CJK glyphs. Use `ui.RegisterFont` before creating views. Freeze identical TTF/OTF bytes in both harnesses. Do not register a new family per render. Public mygo Font API has weight/features but no explicit arbitrary `opsz` axis setter found in this audit. Compare automatic optical sizing on each native backend; propose a focused upstream change only if the first slice proves a mismatch. Converting both references to static fonts would alter the baseline and needs an explicitly named fixture variant.

For Chinese, Japanese and Korean, pin licensed fallback files and ordering in fixtures. Measure punctuation, mixed Latin/CJK, emoji, combining marks and line wrapping. Production may use OS fallback; its screenshots form a separate platform baseline. Do not promise identical text from different system font stacks.

Tailwind spacing uses a 4px base but includes half-steps and exact arbitrary values. Convert compiled computed values, not rounded class-name guesses. Standard radii and custom `2lg=10px`, `2-5xl=20px`, notification-card=10px all matter. A medium Button is 36px high with 10px corners and a 20px icon; small is 32/8/18; xs is 24/4/14. These are evidence from source, not universal control sizes.

Keep ordered multi-layer outer/inset shadows, spread, alpha, one-pixel borders and focus-ring offset. For example dropdown shadow is `0 1px 1px / .04` plus `0 4px 4px / .02`; dark scheme shadows differ. Use native Shadow/InsetShadow/gradient/Draw composition only after checking whether repeated calls accumulate or replace the intended layers. Capture any unsupported composite as a measured renderer gap. Existing OS glass is not a substitute for BoardUI's own surface treatment.

Extract Remix glyph geometry and custom chevrons exactly. Remix uses filled SVG paths on a 24-unit viewBox; do not redraw it with the current Kit's 1.5/1.8/1.9 skin-dependent strokes. Size icons per component. Parse approved SVG once per immutable glyph. Color at render time. Keep the finite glyph cache independent of transient row IDs.

### Motion and scrolling

The newer skill's motion reference is a recipe source, not proof that every recipe exists in the older export. Validate each recipe against that snapshot before coding it. A concrete conflict exists: the skill forbids press scaling, but pinned `theme.css` lines 863–883 define `button-press-motion` with active scale 0.98. Exact-source fidelity takes precedence over mixing recipes from different versions. A no-shrink port must identify a newer source baseline or an explicitly approved divergence.

| Interaction | Reference target | Native mechanism / remaining check |
| --- | --- | --- |
| Hover/press | Skill says 150ms colors and no shrink. Pinned `theme.css` actually has active `scale(.98)`, 220ms press transform and 420ms return | Version conflict: reproduce pinned CSS with `AnimateColor` and explicit pose/easing, or obtain a newer fixed official source before choosing no-shrink |
| Popover/select | 150ms, opacity + scale .95 + blur 2px; placement origin | `ElementTransition`, `Motion`, `CubicBezier`, `AttachedAnchors`, `TransformOrigin`; nesting and exit hit testing must pass |
| Tooltip | Immediate open/close delay by default; 200ms scale .9, blur 4px; 10px offset | `TooltipBase`, negative `Theme.TooltipDelay` for immediate opening and custom surface; source overrides React Aria delay to 0 |
| Modal | Newer recipe 300ms, cubic(.32,.72,0,1), scale .85, blur 4px | `DialogBase` plus presence/Transition; generic Modal absent in fixed export; settings modal must be audited separately |
| Switch/check | 200ms thumb/color; check dash cubic(.65,0,.35,1) | Base state + animated drawing; no opacity-only substitute for stroke draw |
| Rows | Enter 400ms, exit 225ms; tight springs | `ElementTransition` asymmetric exit, `SpringOptions` explicit mass/stiffness/damping |
| Data | Number crossfade 220ms; bar 360ms; cell 380ms; decorative reveal 1800ms | Capture fixed time checkpoints; no count-up as a substitute for crossfade where source differs |

mygo already has `Animation`, physical spring parameters, `CubicBezier`, `Motion.Blur`, separate ExitDuration/ExitEase and `Preferences.ReduceMotion`. Reduced-motion defaults are not identical: mygo can retain non-motion fades while CSS guards may disable all keyframes. Explicitly match each component's final state and timer cancellation. Do not assume the global preference is enough.

BoardUI's public agent rules specify stationary top scroll fades: masked 1px and 4px backdrop blur layers, opacity ramp over the first 24px of scroll, header outside scrolling content. mygo exposes masks, material painting and the existing Kit has `glass.Blur`; content `.Blur` alone does not sample the backdrop. Prove clipped/material coordinates and native GPU cost in a later slice. Mouse wheel, trackpad, touch, scrollbar drag and RTL scroll offsets need separate checks.

### Extraction and drift pipeline proposal

Extend the existing `app/desktop/internal/uikit/scripts/` generation workflow rather than introducing runtime JSON configuration. `gen_tokens.py` currently consumes `themes/default.json` for three OS skins and emits `tokens_gen.go`. Preserve its output. Add a BoardUI source adapter and typed BoardUI output in the native package only after approving implementation.

1. Input lock: official source SHA, each source SHA256, approved license identity, exact Tailwind compiler/dependency lock, font and icon hashes.
2. Parse CSS with a maintained CSS AST parser, such as PostCSS already used by the official reference build. Parse TS with TypeScript. Do not use regex to resolve nested CSS, `@theme inline`, repeated dark rules or calc expressions.
3. Compile the real reference CSS. Export computed semantic values for light/dark/accent fixtures, 52 composite type styles, geometry, ordered shadows and motion records. Preserve alias provenance as well as resolved values. Missing token, alias cycle, unsupported color or duplicate effective definition fails generation.
4. Generate typed immutable Go tables/enums and an asset manifest. Do not generate component behavior from Tailwind classes. No manually maintained parallel token map.
5. Source update job proposes an item/token/enum/asset/license diff. Refuse automatic promotion after license/tier changes or unknown source files. Review affected golden states before updating accepted hashes.
6. Store only approved notices, minimal lawful resource inputs and versioned evidence manifests. Keep npm/source caches outside tracked files. No Pro material enters public fixtures or archives.

## 5. Concrete behavior and native capability matrix

API presence is source evidence only. It does not prove Windows, macOS or Linux native behavior. mygo evidence is from `ui/api_gen.go` plus implementation files named below at commit `9233663736ca`. Primary mygo pointers: [public API](https://github.com/egoist/mygo/blob/9233663736ca/ui/api_gen.go), [Base control implementation](https://github.com/egoist/mygo/blob/9233663736ca/ui/base.go), [motion](https://github.com/egoist/mygo/blob/9233663736ca/ui/motion.go), [transitions](https://github.com/egoist/mygo/blob/9233663736ca/ui/transition.go) and [text input](https://github.com/egoist/mygo/blob/9233663736ca/ui/textinput.go). Local reuse references: [Kit conventions](https://github.com/UniClipboard/UniClipboard/blob/ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0/app/desktop/internal/uikit/doc.go), [Kit factory](https://github.com/UniClipboard/UniClipboard/blob/ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0/app/desktop/internal/uikit/kit.go), [token generator](https://github.com/UniClipboard/UniClipboard/blob/ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0/app/desktop/internal/uikit/scripts/gen_tokens.py), [icons](https://github.com/UniClipboard/UniClipboard/blob/ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0/app/desktop/internal/uikit/icons.go), [0013 proposal](../decisions/proposed/0013-mygo-uikit.md) and [0013 receipt](0013-mygo-uikit.md).

| Requirement | Real API / implementation | Existing reuse and gap | Required first failing scenario |
| --- | --- | --- | --- |
| Roles, names, validation relations | `.Role`, `.Label`, `.LabelledBy`, `.DescribedBy`, `.Invalid`, `.Busy`, `.Controls`, `.ActiveDescendant`; `access.go` | Existing Kit uses Base widgets. Build explicit labels/hints/errors. Verify framework state maps to platform AX/UIA/AT-SPI. | Missing/duplicate name; invalid hint announces wrong field; disabled still activates |
| Tab and roving focus | `ButtonBase`, `TabsBase`, `RadioBase`, `FocusGroup`, `.Focusable`, `.FocusVisible`; `base.go` | Preserve each control's key model; click focus differs from keyboard focus. | Tab enters once; arrows/Home/End move correctly; focus ring not clipped |
| Dialog focus and restoration | `DialogBase`, `.Modal`, `.Focus`, overlay shortcuts; `base.go`, focus/overlay machinery | Generic close sets open=false; exiting render copy takes no pointer/focus. Explicit logical/visual state ownership required. | Nested select Escape closes only top overlay; dialog restores original trigger after exit |
| Popup collision/nesting | `PopoverBase`, `SelectBaseOpen`, `AttachTo`, `AttachedAnchors`, `MenuBase` | Supports anchor flipping; React Aria has offset/crossOffset/collision policies to compare. Kit macOS Select normally routes to system menu. | All four viewport edges; resized/scrolled anchor; submenu safe movement; no swallowed outside action |
| Tooltip timing and description | `TooltipBase`; `Theme.TooltipDelay` and internal warm-window logic (zero means 600ms; a negative value means immediate); `tooltip.go` | Source default delay=0 and closeDelay=0. mygo documentation distinguishes tooltip from accessible Description. | Keyboard focus announces description; hover/open/exit timer reentry; Escape |
| Text/IME/selection | `TextInputBase`, `TextAreaBase`, `.OnPaste`, `.OnTextKey`, `.TextSelection`, `.SetTextSelection`, `.Composing`; `textinput.go`, editor/native text clients | Reuse mature editor, never build a key-to-string editor. Browser autofill and password manager integration have no demonstrated native equivalence. | Composition Enter does not submit; grapheme deletion; shift selection; paste; focus lost mid-composition |
| Controlled/default state | Explicit app state pointer or `c.State` with stable keys | Map value/defaultValue initialization once; do not reset state each frame. Keep focus/editor state widget-local; domain data caller-owned. | Parent update while focused; remount keyed control; async response arrives after teardown |
| Validation/busy/async | `.Disabled`, `.ReadOnly`, `.Invalid`, `.Busy`, progress/status roles | Model busy separately from disabled. Cancel work at owner teardown; stale completion cannot mutate new instance. | Double submit, timeout, server error, canceled dialog, changed selected item |
| Table listing | `Table`, `ListState`, `TableColumn`, `SortOrder`, virtual list (`table.go`) | Sorting, resize/reorder and row selection exist. The styled header is a fidelity risk; Kit.Table is not a full port. | Selection survives sorting/filtering via stable IDs; virtual indices/counts; empty/loading/error/page boundary |
| Spreadsheet grid | `ListState.CellSelect`, `ColumnRange`, cell-selection machinery (`cellselect.go`) | Fixed official DataGrid has its own virtual rows and editing/TSV/undo logic. Do not claim TanStack DataTable covers it. | Cross-cell paste, range extension, edit commit/cancel, undo after reorder, offscreen selection |
| Date/time/number | `DateInput`, `Calendar`, `.Browse`, `TimeInput`, `NumberInput` | Real reusable editors. Two-month range, unavailable dates, calendar system and localized number behavior need comparison. | Locale segment arrows/Home/End; negative/decimal; week starts; DST and range crossing month |
| Drag/resize/file input | `.Draggable`, `Drop`, `DropData`, `DataDragOver`, `.DroppedFiles`, table columns, Split | Native transfer and host file picker are separate from app-internal drag. Pro kanban source uninspected. | Canceled drop, empty payload, pointer capture loss, touch drag, keyboard resize and permission failure |
| Responsive/RTL/touch | Context direction, logical anchors, ScrollState, measured layout, native pointer events | Breakpoints come from compiled Tailwind defaults/overrides. Mirror directional glyphs only; keep plots/code/OTP physical. | 360/768/1280 widths; 1/1.25/1.5/2 scale; CJK expansion; touch scroll against nested overlays |
| Native accessibility | mygo access tree and platform bridges | JS React Aria supplies semantic intent, not Go behavior. Preserve public names/values/actions; do not mimic DOM node names. | VoiceOver/UIA/AT-SPI reads same name/value/state and operates selected controls |

An independent native package needs in-window BoardUI-styled popups for visual equivalence. This conflicts with the existing Kit's intentional macOS system-menu convention. Keep the proposal isolated; changing production menu policy requires a separate explicit adoption decision. It must not be silently changed by this research.

## 6. Native package and state architecture

| Route | Benefit | Concrete cost | Recommendation |
| --- | --- | --- | --- |
| Add BoardUI to current `Kit{Skin,Dark}` | One factory and call site vocabulary | Skin-specific internal branches; platform-dependent geometry; only 59 original public methods; native menu routing; press feedback; no complete date/grid/agent suite | Suitable only if accepting approximation. Does not satisfy the requested invariant by itself. |
| Reusable Go BoardUI package using mygo | Preserves source vocabulary, native behavior, independent versioning and exact style | One explicit product adapter; license/resource ownership; API must be proven on slice | Preferred for high fidelity. Start isolated, do not migrate product pages during library research. |

Proposed smallest package: one native component package, generated resources beside it, and one fixture application. Do not create a separate theme service, virtual DOM or generic event bus. Place the first trial under the desktop boundary. Keep it independent of UniClipboard domain DTOs and internal Kit types so it can be extracted to its final Go module after the slice proves the API. That is one implementation moving to its final distribution location, not concurrent replacement libraries. Decide the publication location before committing substantial library code.

Use a small value factory containing an immutable resolved theme. Methods take `*ui.Context`, typed Props and explicit state; return `ui.Element` or small interaction results. Preserve named BoardUI variants/sizes. Use mygo comparable state keys and stable item IDs. Expose optional selected values explicitly rather than encoding no-selection as a valid first item. Do not mirror React callbacks mechanically or export DOM class strings as a Go styling API.

Generated resources own token IDs, composite text styles, motion records and approved glyphs. Components own presentation and widget-local state. The host owns windows, DPI, native font registration, clipboard, URL opening, file pickers and preferences. Caller state owns forms, remote requests, data sorting/filtering and cancellation. Renderer-specific paint gaps belong upstream in mygo only when a reproducible case demonstrates them. Core Engine and CLI/service dependency graphs must remain UI-free.

Theme switching swaps the resolved theme for the next frame without rebuilding form/domain state. Include theme generation in paint caches that depend on colors. Keep glyph geometry caches theme-independent. Host persistence is optional policy, not a library concern. The reference theme-toggle's localStorage/click-origin reveal should become an explicit optional behavior; no hidden system-theme policy in controls.

## 7. Performance design and measurement

No memory, first-frame or CPU comparison was run. No claim of a reduction is justified. Preserve existing separate GUI/panel/tray/service processes. A React reference is an isolated acceptance fixture, not a production dependency. No Windows VM, production GUI, system clipboard or personal profile was used.

Measure identical synthetic data and window/DPI states: cold launch, first meaningful fully-font-loaded frame, warm idle, 100 open/close cycles, theme switching, 10k-row scroll, sustained stream and return to idle. For Windows record process tree private bytes, working set, peak commit, handle count, GPU dedicated/shared memory and frame latency. For browser reference count browser/renderer/GPU processes and disclose Next server separately. For mygo count the fixture process and renderer resources; production role totals are a separate scenario. Heap-only or renderer-only numbers are not comparable total memory.

Pin release-build flags, renderer backend, font files, OS image, browser version and acceleration. Run multiple samples, record distributions and warm/cold distinction. Retain first failures. A headless software screenshot cannot establish native GPU costs or Windows resident memory.

| Resource | Proposed ownership/bound | Evidence to collect |
| --- | --- | --- |
| Parsed SVG | Finite approved glyph IDs per package; no row-ID keys | Count/cache bytes stabilize after exercising every glyph |
| Font registrations and glyph atlas | Register once per process; finite families/weights; inspect renderer eviction | CJK/emoji scroll peak and stable post-scroll plateau; no repeated font copies |
| Blur/shadow surfaces | Bound by visible popup/panel pixel area and scale; no permanent full-screen layers per component | GPU allocation and frame cost at 1x/2x; entering/exiting surface release |
| Lists/tables | Reuse virtualization; keep data outside widget nodes; cap visible/overscan work | Created node count scales with viewport; 1k vs 10k data comparison |
| Animations | Finite channels keyed by live widgets; cancel timers on unmount and reduce-motion | Idle returns to no scheduled animation frames; 100-cycle memory trend |
| Thumbnails/chart geometry | Cache by immutable data revision/viewport; bounded entries | Resize/period/theme churn does not grow without bound |

Set numerical budgets after first-slice measurements on target hardware, then freeze them for regression. Do not invent a megabyte target or delivery estimate from component counts alone.

## 8. Reproducible high-fidelity E2E specification

Before implementation, inventory these failure classes: wrong version/license/tier; mutable registry; incomplete CSS compilation; missing font/opsz/fallback; mismatched viewport/DPI/locale; baseline captures before font load; changed text wrapping; rounded geometry drift; incorrect icon glyph; wrong shadow/alpha/color space; skipped interactive state; hover masquerading as keyboard focus; swallowed outside click; broken nested focus; lost IME selection; stale async state; unbounded caches; software-only result reported native; stale or incomplete artifacts reported PASS.

First create and exercise the official React fixture from the real pinned source and its dependencies. Do not hand-write React substitutes. The registry is not needed for components already present in the official export, but export and live-registry fixture identities must stay distinct. Build a lockfile with exact dependencies, record package integrity, freeze font files and compile with the exact pinned Tailwind. The export itself has no lockfile; the first fixture build remains a prerequisite, not an accepted result of this research.

Use one synthetic fixture schema for component ID, props, stable data IDs, actions and expected states. It is a test fixture, not a production configuration layer. Serve local fonts/assets. Disable analytics, provider requests and network; no agent-runtime calls. Preload offline legal fixtures into a content-addressed acceptance cache. Missing files must fail rather than silently reaching the network. Pro fixtures require separate lawful/private storage and must never be copied to a public report.

### Dimensions and state expansion

Every item in the machine catalogue expands into its real variants × sizes × applicable state cases. Use a coverage manifest with required combination IDs; no silent skipped states. Do not demand logically impossible combinations. Pairwise selection may reduce redundant long interactions only after all individual visual variant/size/state combinations are covered. Every unsupported state is explicit BLOCKED or NOT_RUN.

Common visual states: resting, pointer hover, pressed, keyboard focus-visible, pointer-focused, disabled, selected/mixed, required, invalid with hint/error, read-only, busy/loading, empty/error and long/multiline content where applicable. Capture each supported icon placement and icon-only naming case. Add light/dark, default/changed accent, LTR/RTL and English/zh-CN/ja-JP/ko-KR representative text. Full accessibility and text-entry locale scenarios are separate from static image permutations.

For Button alone, four variants × three sizes × four documented CSS states already produce 48 combinations before icon forms and focus. For Switch, the source has three sizes, despite a short catalogue description saying two. Use source domains, not marketing prose, to size work. The count of full-library permutations cannot be finalized while 18 free source entries and Pro APIs remain unverified.

Set fixture content box to 1280×800 CSS px/DIP by default and include 360×800 and 768×800 responsive cases. Use explicit native client area, not outer decorated size. Sweep scale 1, 1.25, 1.5 and 2. Freeze locale, direction, timezone, device scale, sRGB profile, identical font bytes, data, timestamp, accent, scroll position and reduced-motion mode. Keep device-pixel screenshots and DIP geometry side by side.

### Objective comparison gates proposed for the first slice

These thresholds are proposed gates to validate and freeze on the first slice, not measured achievements:

1. Compare DOM layout/computed-style measurements to mygo element rectangles, padding, baseline and popup anchors. Require edges/centers and text baselines within 0.5 DIP. Record final rounded device-pixel positions. Require identical wrapping, row count and truncation behavior.
2. For flat non-text fills/borders/icons in the same capture color space, require per-channel error ≤2/255 except a one-device-pixel antialias boundary. Compare icon path/bounds separately. Require exact token and geometry manifests even where raster output differs.
3. Produce side-by-side PNG, alpha overlay, absolute-difference heatmap and region metrics. Compute thresholds per component region, not page-wide blank space. Never translate/resize images automatically to hide layout errors.
4. Treat font antialiasing and OS compositing as separately labeled regions. Calibrate using repeated captures of the same reference and font-only fixtures on each OS. A proposed raster tolerance can cover edge intensity, not a different font, missing glyph, weight, baseline or line break. No blanket text masking.
5. For shadows/blur, compare sampled profiles, affected bounds and alpha falloff; retain unmasked difference metrics. Establish renderer-specific tolerances before accepting them. A failing composite cannot be hidden under an overall similarity percentage.
6. Drive real keyboard sequences and semantic accessibility actions. Assert names, roles, values, disabled/busy/invalid state, focus target, selection and cancellation. Home/End/arrows/Escape tests must assert the final state, not just that a key was sent.
7. Capture motion at controlled 0/75/150/200/300ms and settled time as applicable. Bind a deterministic clock where supported. Add native continuous recordings; do not synthesize videos from checkpoint frames. Reduced-motion captures must reach correct stable states without lingering timers.
8. Report each checkpoint PASS/FAIL/NOT_RUN with source identities. A visual mismatch can pass only through an explicit reviewed exception with reason, affected pixels/behavior and scope. User visual approval is distinct from automated gate success.

Charts are a separate track. Compare data-to-domain transforms, axis ticks/labels, interpolation and stacked baselines, gradients, legend ordering, hit regions, hover isolation, keyboard/data alternative and animated checkpoints. `ui/chart` exposes mature geometry/scales/stacking/downsampling; that is not proof of all Recharts cards, Sankey layout, heatmap interaction or the surrounding card UI. Test invalid/missing data and extreme ranges. Freeze fixtures for each licensed card rather than advertising one generic chart widget as coverage.

### Harness integration and artifact identity

Follow the existing [Desktop E2E contract](../contracts/desktop-e2e-contract.md). Reuse the canonical `make desktop-e2e` catalogue/runner and evidence machinery when the slice is implemented. Add explicit BoardUI cases before component code. Proposed IDs: `boardui-settings-reference`, `boardui-settings-view`, `boardui-settings-native` and later `boardui-table-native`. These cases and commands are NOT IMPLEMENTED by this research.

The retained artifact must contain: source lock and clean/dirty identities, per-input hashes, browser/Go/mygo/OS/renderer versions, fixture schema/version, fonts/icons, dependency lock, commands, measured geometry/style JSON, semantic tree assertions, event timeline, PNG pairs/overlays/diffs, recording for native cases, metrics, report and SHA256SUMS. Freeze source before build. Recheck hashes before teardown. Remove binaries, profiles and raw logs after owned processes exit. Keep only lawful synthetic acceptance evidence under the existing retention policy.

Preserve the first failing artifact. Bound retries to two additional attempts or a fixed investigation budget. Compare failures. Do not overwrite goldens after seeing a failure. A recurrence or unclear relevance blocks delivery, as required by the repository's failure rule. Passing retries do not erase the original failure.

## 9. Phased route and runnable exits

| Phase | Exact scope / dependencies | Runnable exit and objective gate | Blocking questions |
| --- | --- | --- | --- |
| R0 — this research | Source/version/license/API inventory | Documentation, 127-row evidence union, hashes and proposed E2E specification; no UI port | Live registry 402; 18 free APIs not verified; Pro distribution permission absent |
| P0 — reference first | MIT export only; exact dependency lock; approved fonts/icon subset; synthetic form data | Run official React Button/Input/Select/settings-dialog fixture offline; deterministic screenshots/geometry/semantics and checksums | No floating font download; distinguish settings-modal from missing generic modal |
| P1 — minimum native slice | Button + Input + Select + an existing pinned settings-modal composition; minimal shared text/surface/icon resources | Native form opens, edits/validates, opens Select inside dialog, closes/restores focus, switches theme; all required image and behavior gates pass | Decide public/private library location; confirm in-window popup policy for trial |
| P2 — complete basic controls | Accepted slice API; checkbox/radio/switch/cards, tabs/segmented, textarea, tooltip/menu; base display/navigation primitives | Each component exits with its own complete state manifest; form slice continues passing | Range input, font axes and native accessibility gaps may need mygo upstream work |
| P3 — data/date/complex inputs | Stable row IDs, Table/ListState, locale decisions; verified missing-source items if desired | Sort/filter/select/page/resize 10k-row table; DataGrid edits/TSV/undo separately; date/range/time fixture; bounded resource evidence | Header styling seam; locale calendar behavior; touch/native platform coverage |
| P4 — application/agent and free charts | Accepted primitives; offline synthetic streams; actual public source | Agent transcript/composer/thinking flow and free revenue/orders cards pass dedicated interaction, chart and performance gates | Streaming ownership, accessibility verbosity, native renderer costs |
| P5 — Pro/templates and product adoption | Written rights grant where needed, exact licensed sources, accepted contained components, separate product approval | Each template is a composition E2E, then one separately authorized product page integration | No automatic all-product replacement, VM use, publication or release |

P1 is one vertical slice. Do not implement all components before comparing the first slice. If generic Modal source is still unavailable, use the actual pinned Settings Modal as reference; do not invent a React generic Modal. Its Go host behavior must preserve the official composition and named differences. A smaller custom native dialog would be a demonstration of primitives, not acceptance of the generic BoardUI Modal.

Effort must be estimated after P1 from measured work categories: source review, generation, native paint, state logic, native behavior gaps, E2E scenarios and platform exceptions. Use the audited 55 pinned visual/application install items and 14 foundations as the initial denominator. Exclude unrelated agent-runtime server code from native widget implementation. Add missing free/Pro items only after source and license review. The 30 Pro components and 10 templates have no source-based estimate. Counts do not justify a calendar promise.

Potential upstream work is evidence-triggered: font variation control if optical sizing differs; style seam if the native table header cannot match; nested popup/focus/collision defect if reproducible; native accessibility action gaps; masked backdrop material parity. Existing mygo already covers many of these categories in part. Do not rewrite them preemptively or upgrade the fixed dependency without a separate bounded change and E2E evidence.

## 10. Research verification receipt

Performed: repository/origin/branch verification; fetch and baseline match; read RTK, root and desktop guidance, overview, 0013 proposal/receipt, Engine boundary guidance and Desktop E2E contract; read BoardUI skill plus components/theming/motion/patterns references; anonymously retrieve public npm metadata; verify tarball integrity and hash; run real CLI list with isolated XDG config and telemetry disabled; clone/read/hash official public source; TypeScript AST catalogue extraction; audit fixed mygo APIs and current Kit; inspect primary license references.

NOT RUN: React fixture build/render, native Go port, visual E2E, keyboard/IME/native accessibility E2E, memory/first-frame benchmark, Windows VM work, production application or clipboard interaction. No UI behavior was changed. No unit tests were added. No push, PR, commit, merge, purchase, plugin installation or publication was performed.

Public source, downloaded npm dependencies and caches were isolated under ignored `.build/boardui-research/` and removed after generating manifests and verifying documents. Retained files are this English report, matrix and small metadata/hash catalogues. Planning notes remain ignored. Documentation validation results are appended below after execution.

### Replay the research identity checks

Run from this UniClipboard worktree. These commands retrieve only public source into a separate ignored directory. They do not start a GUI or install a component into the product. They verify the pinned export rather than promising that an unavailable live registry will now succeed.

```sh
mkdir -p .build/boardui-research-replay
git clone --no-checkout https://github.com/BoardUI/boardui.git .build/boardui-research-replay/upstream
git -C .build/boardui-research-replay/upstream checkout --detach ec5c5eb3e29c2441be63b9422cd54f411ef23466
python3 - <<'PY'
import pathlib, json, hashlib, urllib.request, base64
manifest = json.loads(pathlib.Path('docs/exec/boardui-mygo/sources.json').read_text())
root = pathlib.Path('.build/boardui-research-replay/upstream')
for entry in manifest['boardui_git']['files']:
    data = (root / entry['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == entry['sha256'], entry['path']
cli = urllib.request.urlopen(manifest['cli']['tarball'], timeout=30).read()
assert hashlib.sha256(cli).hexdigest() == manifest['cli']['sha256']
assert 'sha512-' + base64.b64encode(hashlib.sha512(cli).digest()).decode() == manifest['cli']['integrity']
print('Pinned public source and CLI integrity verified')
PY
(cd docs/exec/boardui-mygo && shasum -a 256 -c SHA256SUMS)
```

For a later CLI availability recheck, install **CLI 0.5.9** under a disposable prefix with `--ignore-scripts`, use a private `XDG_CONFIG_HOME`, unset license/registry overrides and set `BOARDUI_TELEMETRY_DISABLED=1 DO_NOT_TRACK=1`; run its actual `dist/index.js list`. Do not interpret a changed live response as the same research snapshot. Do not use `add --all`, login or Pro endpoints to reconstruct this report.

Checks executed in this research: CLI tarball SHA512 integrity PASS; 127 unique catalogue IDs PASS; all 69 pinned entry-file SHA256 hashes PASS; `go run ./tools/docscheck` PASS with six existing stale-architecture warnings (headless, identity-and-trust, observability, overview, real-machine-testing, releases). No architecture verification stamp was advanced because this work changes no behavior. Local report links and retained checksums were validated separately.
