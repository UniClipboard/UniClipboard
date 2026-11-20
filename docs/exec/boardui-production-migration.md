# Production Desktop migration to native BoardUI

Status: production implementation and software acceptance complete; local delivery only. The old Kit implementation and platform skin selector are removed. Native-platform execution remains NOT_RUN. Final results are appended below and bound by a small machine-readable receipt.

Planning baseline: UniClipboard commit `35b1d56b52e4c5fdb210455fcbac00cd9831d8bf`, branch `feat/boardui-mygo`. BoardUI source remains the official MIT export `ec5c5eb3e29c2441be63b9422cd54f411ef23466`. mygo remains `v0.8.4-0.20261108082913-9233663736ca` unless a separately justified change is recorded. The user has authorized one BoardUI presentation across macOS, Windows and Linux, production adoption and removal of the previous shared UI solution. Host OS integration remains native.

## Failure inventory before migration

- A directory rename leaves the old three-skin design, token tables or platform overrides active.
- A compatibility alias or fallback silently selects the old UI Kit after apparent migration.
- A migrated package builds in isolation while setup, startup errors, preview or quick-panel roles still import the deleted package.
- BoardUI's four accepted controls are mistaken for coverage of the entire production interface.
- A missing control uses an unchanged mygo default widget and is presented as an accurate BoardUI port.
- Old 22-point macOS field assumptions clip the 32/36-point BoardUI field, hint, error, focus ring or pointer target.
- Platform-specific button scaling, icon strokes, glass effects or menu rendering remain in application content.
- Buttons lose their disabled/busy behavior, required labels or safe destructive defaults.
- New input, search or code-field compositions lose IME composition, caret/selection, full-width digit handling, password secrecy or keyboard completion.
- A Select migration retains a swallowed outside action, wrong initial selection, broken Escape scope, missing typeahead disclosure or missing trigger focus restoration.
- Sheet/dialog replacement changes dismissal ownership, destroys active content during exit or restores focus before the overlay releases it.
- List/grid/table migration loses stable IDs, selection, viewport ranges, scroll anchors, paging, fixed-column behavior or bounded detail caching.
- Tag assignment, rename/merge/delete, schema forms, settings CAS/reload or service reconnect changes behavior while only the new appearance is checked.
- Theme changes reset caller-owned state or preserve an OS accent/skin override instead of the locked BoardUI semantic theme.
- Native caption controls, traffic-light insets, resize edges, drag regions, maximize state or close lifecycle change with a content-style refactor.
- Quick Panel changes focus/paste target identity, physical-input ordering, global shortcuts, tray adapter ownership or its process lifecycle.
- Core Engine or UI-free service/CLI programs acquire a transitive UI dependency.
- A screenshot runner uses a real profile, system clipboard, personal data or another task's native environment.
- A CSS-pixel reference image is compared against device pixels, a different font/locale/data is used, or images are resized to hide a mismatch.
- Old tests are removed before their behavior assertions exist in the new E2E surface, or a first failure is overwritten by retries.
- Windows/Linux software-view screenshots are reported as native platform acceptance or a memory reduction is promised without measurement.

Retain the first failure artifact. Allow at most two additional attempts for the same unchanged scenario. Diagnose a repeated or relevant failure before proceeding. A passing retry does not erase the initial result. Write the production E2E scenario and expected failure before changing the corresponding behavior. Do not add unit tests after implementation.

## Fixed target and package ownership

Use one source-derived BoardUI visual language for application content on all three OS targets. Keep scheme selection (`system`, `light`, `dark`) separate from OS skin selection. Remove the Adwaita/macOS glass/Fluent selector, its generated platform palettes, platform typography, hand-redrawn icon variants and production `UNIC_UI_SKIN` path when their final consumers migrate. Do not provide an old-skin fallback or alternate production theme that keeps the previous system alive.

`app/desktop/boardui` owns source-bound reusable controls, theme/type/geometry/motion resources and approved glyphs. `app/desktop/internal/desktopui` continues to own product state, service binding, history/search/tag/device/settings composition and async cancellation. An application-internal `desktopkit`, if used to retain existing composition APIs during the conversion, must consume the same BoardUI resources and primitives. It must not become a renamed copy of the former skin engine or a third design system. Keep product-specific DTOs and behaviors out of the reusable public package.

Migrate runnable slices in place. Existing unmigrated callers may continue compiling against the old package while the branch is under construction. Complete every caller conversion before deletion. Do not create a general renderer abstraction, duplicate text editor, generic callback bus or runtime configuration layer to bridge the migration.

The first-slice [contract](../contracts/boardui-native-contract.md) and [receipt](boardui-mygo-implementation.md) establish Button, Input, Select, a bounded derived Dialog and their helper resources. They do not establish production-page coverage. Their recorded limitations, including Select typeahead and native platform acceptance, remain applicable until separately resolved and verified.

## Source inventory at the planning baseline

The old package is `app/desktop/internal/uikit`. A source scan at the planning baseline found 26 non-test Go files outside that directory importing it: 16 in `desktopui`, five in the production command, one in setup UI, one in window chrome, one in the demo launcher, one in preview DTOs and one in demo fixtures. Two additional external Go test files import it. These are initial inventory observations, not final deletion counts. Recompute them after each slice.

The historical 0013 reference declared 59 public components. The current old package has 80 exported `Kit` methods, including helpers added later. Neither number is the production migration checklist. Convert the actual consumer graph and preserve its behavior. The observed caller vocabulary includes basic controls, settings rows, token search, code/combobox fields, menus, sheets, virtual lists/grids/tables, previews, navigation and product status compositions.

| Consumer / capability | Proposed migration | Required behavior to preserve |
| --- | --- | --- |
| `cmd/unic-desktop`, `session_gui`, legacy settings/history helpers | Import the approved BoardUI-backed application composition; remove every old Kit import | Startup error/retry, service reconnect, settings persistence and device approval controls |
| `setupui` and first-install/migration pages | Recompose with BoardUI Input/Button/type/surface and required status/progress controls | Profile-owned setup state, safe cancellation, v1 importer ownership, credential secrecy and explicit completion |
| `desktopui` history, detail, devices and settings | Convert one runnable page path at a time while preserving the existing domain models | Live service behavior, stable selection, optimistic state/CAS, recovery and approval fingerprints |
| Search/token search, tag editor and plugin schema forms | Use source Input/Select/Dropdown shapes with mature mygo editors and existing domain models | Caret-preserving token edits, candidate selection, tag colors, validated schema values, write-only secrets |
| List/grid/table navigation | Retain mature mygo virtualization and selection machinery under BoardUI paint/composition | Visible ranges, paging triggers, bounded caches, scroll anchors, fixed columns and context actions |
| `desktopdemo`, fixtures and preview DTOs | Use the same migrated product composition; replace presentation-only old types | Synthetic fixtures remain isolated; preview behavior does not create product integration |
| Quick Panel and satellite windows | Apply BoardUI content resources independently of the OS focus/paste host | Target identity, owned process roles, delayed startup, selection, keyboard navigation and bounded rendering |
| `windowchrome` | Retain a native-host package; remove its old Kit dependency | Actual OS window actions, traffic lights/GTK reservations, drag regions and close lifecycle |

## Resource and API mapping rules

The current native package generates 74 semantic/accent colors per scheme, 52 complete typography records, seven size records, button gradients/shadows, Select timing and six glyphs. It is sufficient for the first slice, not every production status, menu, tag, progress or layout. Extend the existing official-source adapter for required public resources. Missing source or license is a blocker for that claimed official component; it is not permission to invent one or fetch Pro source.

| Previous vocabulary | BoardUI target / explicit choice | Constraint |
| --- | --- | --- |
| `Sem("background")`, content surface | `background-full` or `background-primary-default` according to the actual source composition | Do not map every surface to the same old platform window color |
| `foreground`, `mutedForeground`, text tones | `text-primary`, `text-secondary`, `text-tertiary`, `text-error-primary` | Preserve disabled/error distinctions and contrast |
| `primary`, `primaryForeground` | Source primary gradient and `text-white`; accent ramp for appropriate selection/focus roles | Do not replace the source gradient with one flat OS accent |
| `secondary`, `muted`, `accent` | Source `background-*-default/hover/active` or button-ghost semantics, chosen per control state | A global one-to-one alias would collapse distinct BoardUI states |
| `border`, `input`, `ring` | Source border-button/focus tokens; Input's actual inset state border recipe | Borders, field inset rings and outer keyboard focus rings are distinct |
| `destructive`, positive/warning/chart colors | Source danger/status/chip/chart roles as applicable | Generate missing exact public values; do not reuse a chart series as a generic success color |
| Old OS type roles and page `.FontSize` overrides | Named composite BoardUI typography records | Preserve size, line height, weight, tracking and font together; review large code/invitation text separately |
| Old `SizeLarge` / `SizeXLarge`, `Glass`, toolbar-specific variants | A documented source size/variant or an application layout using accepted controls | Do not silently map an unsupported size or preserve platform glass as a second design language |
| Old icon vocabulary and skin stroke tables | A bounded list of exact approved Remix/custom glyphs with hashes and notices | No platform redraw or per-row cache keys; app icon names may remain semantic |
| Tag palette / custom `#rrggbb` preferences | Source chip/tag appearance plus caller-owned palette/custom value | Preserve stored user intent and model semantics; do not add Engine presentation dependencies |
| Scroll / list factories and popup presence | Existing mature mygo behavior with BoardUI resources and explicit ownership | Keep scroll indicators, overlay lifetime, viewport reporting and focus restoration centralized |

Free source available at the fixed export includes Checkbox/Radio/Switch, Slider, Tabs/Segmented Control, Tooltip/Dropdown, Badge/Chip, Table, Sidebar and several relevant application blocks. Port only required controls before their production use. Newer source-unavailable generic Search Field/Combobox/Modal/Progress names must not be presented as already ported official APIs. Application-specific search, sheets, notices and progress can be honest compositions of inspected public resources and native primitives. Record that distinction in implementation coverage.

Do not confuse the original BoardUI Settings Modal's 871×614 multi-page application with the accepted 480-wide synthetic settings form. Production settings are a UniClipboard composition. Preserve product functionality while adopting the source language; do not claim an untouched official full-page template or a Pro template.

## Native host boundary

Keep `internal/windowchrome` as the host owner. Preserve `Configure`, actual maximize-state queries, minimize/restore/close actions and the existing close handler. Windows still needs caption actions above setup, startup errors and normal pages. Linux/macOS retain the saved system-frame preference and its restart semantics. Keep native traffic lights, GTK-reported caption reservations, drag regions, resize edges, shadows and the OS window-manager contract. BoardUI content spacing must respect these measured insets.

In-content Select, action menus and context actions should use the unified BoardUI presentation after their conversion. Native tray/menubar menus, file pickers, credential prompts, updater windows, notifications, global shortcuts, accessibility bridges and clipboard/paste integration remain host services. A content-style change must not replace them with web or hand-built OS simulations.

OS checks remain valid where they select native integration, process/backend or capability. Remove OS checks that choose content colors, typography, icon strokes, button surfaces or menu skins. A blanket removal of all `GOOS` checks would break native integration and is not the target.

The separate GUI, Quick Panel, tray and service roles remain intact. Keep the tray's exclusive clipboard adapter and existing focus/paste barriers. BoardUI must not acquire Engine/service ownership or a transitive path from UI-free commands.

## Phased runnable exits

| Phase | Scope | Required runnable exit / acceptance | Status |
| --- | --- | --- | --- |
| M0 | Record inventory, failure list and first production E2E expectations before changes | Freeze source and current consumer graph; retain first expected red; agree the actual first production page route | Recorded; initial red retained |
| M1 | Extend only the missing public primitives/resources needed by one production settings path | Settings reads, edits, validates and persists via the real isolated service; reopen preserves values; disabled/busy/errors and field geometry pass | Implemented; isolated service/data-binding checks passed |
| M2 | Convert setup/startup errors and native caption composition | Fresh/established profile routing, error/retry, safe migration cancellation and every caption action remain runnable; native host ownership unchanged | Implemented; software/process evidence passed; native execution NOT_RUN |
| M3 | Convert main history/detail/devices/settings/search/tag/schema paths | Production renderer + real isolated service cases pass for browsing/paging, edits, tags, device approval/removal, schema secrets and recovery; new state screenshots match the locked source language | Implemented; broad view checks and diagnosed reruns recorded below |
| M4 | Convert Quick Panel, satellites and previews | Panel opens, navigates, selects and closes with unchanged process/focus/paste boundaries; shared preview mirrors migrated renderer; no old-style branch remains | Implemented; Quick Panel/view/process boundaries retained |
| M5 | Delete old Kit implementation, old token generator/palettes, skin selectors and obsolete-only test plumbing | Zero active import/path/selector/dependency references; production/demo/headless builds and complete relevant E2E catalogue pass | Removed; static boundary and build checks passed |
| M6 | Complete documentation and platform evidence ledger | Current architecture/contract describe actual production ownership; software/native evidence separated; native gaps explicitly NOT_RUN; no stale old-skin launch instructions | Current documents updated; final receipt pending; native limits explicit |

Do not proceed by changing every import first and then repairing an unbuildable tree. Each slice has a real callable route, validated state transitions and repeatable artifacts. The final target has one production visual system; the staging order is not a permanent dual-kit architecture.

## E2E and evidence requirements

Reuse the canonical [Desktop E2E contract](../contracts/desktop-e2e-contract.md) and existing service/renderer/native case machinery. Add or revise declarative cases before changing their covered behavior. Preserve useful assertions from old tests as E2E scenarios; remove only assertions that solely require retired skin geometry after replacement behavior is specified.

Cover settings persistence/restart-required fields, startup and fresh setup, v1 migration cancellation/reopen, live history/list/grid/search/paging, tag assignment/merge/delete, device trust approval/removal, plugin Schema secret fields, update actions, nested overlays, outside Cancel, Tab/arrows/Home/End/Enter/Escape and focus restoration. Preserve required fingerprints and safe default cancellation. Do not weaken real service assertions to in-memory fixtures because the visual system changed.

Compare real official React component states and native components with identical font bytes, source/dependency hashes, fixture data, locale, scheme, viewport and DPI. Capture raw device pixels. Preserve original PNGs, geometry, source type/color values, semantic assertions, side-by-side/difference images and SHA256SUMS. The first slice's 0.5 DIP control-geometry and 2/255 flat-fill gates remain relevant to those same controls; they do not imply acceptance of newly introduced components, shadows, text rasterization, animation trajectories or OS composition.

Run long/translated text, CJK/emoji editing, narrow windows, 1x/2x, light/dark and reduced motion. Record text fallback and native IME/accessibility limitations rather than marking them passed from software input. Test the actual supported minimum sizes and fixed/scrolling content boundaries. Preserve render/cache bounds and idle wake behavior; compare process metrics with an explicitly named production role and workload. Do not claim Windows memory results from macOS software views.

Native platform work must use an explicitly coordinated environment. Do not take another task's Windows VM, personal profile or clipboard. Until platform scenarios execute, list Windows/Linux/macOS native input, accessibility and compositor results separately as NOT_RUN, regardless of source/build portability.

## Removal and documentation closure checklist

The migration owner must verify all of the following before calling the old scheme removed:

1. Remove `app/desktop/internal/uikit` after all live/test/demo imports and package strings are gone. A renamed skin engine does not satisfy this gate.
2. Remove `SkinID`, `DefaultSkin`, `UNIC_UI_SKIN`, old per-platform palette/typography tables, generated `themes/default.json` resources, platform glyph overrides and old skin-specific content behavior from the active production solution.
3. Migrate old consumer DTOs that accidentally depend on presentation enums. Keep domain content kinds and selection state in the appropriate application/model package; do not move them into Engine for convenience.
4. Recompute dependency graphs for product GUI/demo/Quick Panel and UI-free CLI/service/Engine. Build all relevant binaries. No old package or hidden source copy may remain through an indirect import.
5. Update the E2E catalogue, fixtures, source identity and launch scripts. Replace three-skin permutations with three native-platform evidence groups plus shared BoardUI light/dark scenarios; do not relabel software skins as platform runs.
6. Update the live documentation only after the implementation supports each claimed statement. Preserve prior receipts as historical evidence, with a superseded pointer if required. Do not rewrite earlier results as if they accepted BoardUI production pages.

### Clauses requiring owner updates

These are recommendations for the root migration owner. This task modifies none of the listed architecture, contracts, AGENTS or README files.

| Document / section | Existing clause to retire or revise | Required replacement / preserved invariant |
| --- | --- | --- |
| `docs/decisions/proposed/0013-mygo-uikit.md`, decisions 1, 2, 4 and approximation section | Old 59-component contract, `Kit{Skin,Dark}`, platform preview selector, `themes/default.json`, accepted approximate icons/glass/menus | Record a superseding decision after checking the latest decision number. BoardUI source lock, one cross-platform content style and objective per-component acceptance replace those choices. Keep native primitives, caller state and core/UI separation. |
| 0013 layout constraints | macOS 22pt/5pt/6pt TextField geometry and skin focus ring | Replace content geometry with actual source sizes. Preserve unclipped focus rings, safe shrinking, viewport-centered empty state, stable segment layout and explicit approval/destructive safety. |
| `docs/exec/0013-mygo-uikit.md` | Old implementation/validation and three-skin screenshots | Preserve as a historical receipt. Add supersession context instead of deleting valid past evidence or treating it as new acceptance. |
| `docs/architecture/overview.md`, Desktop layout demo | Adwaita/glass/Fluent ownership; platform glass press motion; macOS NSMenu and sheet rules; platform token-search styling | Describe actual BoardUI composition/resources and native host split. Preserve search semantics, paging, bounded caching, table behaviors and dialog focus/lifetime invariants. Do not blindly preserve retired visual numbers. |
| `overview.md`, setup, i18n, tag and native BoardUI first-slice sections | Old Kit owner, three-skin language acceptance, 170×22 macOS tag field, production does not import BoardUI | Update actual production imports, shared type/geometry and platform evidence. Preserve translation catalogues, tag model behavior and accepted limitations. |
| `docs/architecture/desktop-runtime.md`, main-window decoration and independent component acceptance | Old Kit draws caption actions; standalone-only BoardUI and runtime-config exclusion | Attribute caption behavior to native `windowchrome`, content to BoardUI-backed composition. Preserve window-frame restart behavior, process roles and service ownership. |
| `docs/contracts/boardui-native-contract.md`, scope/behavior/coverage | Existing production pages/three-skin Kit retain interfaces; production menu policy unchanged | Update scope only for implemented production adoption. List newly exposed controls/compositions and remaining unsupported features. No implied Engine/service API changes. |
| `docs/contracts/history-contract.md`, status; `docs/architecture/identity-and-trust.md`, presentation wording | Older generic Kit/retired client presentation statements | Update presentation ownership only. Keep all history IDs, paging, leases, content application and trust/approval semantics. |
| Root `AGENTS.md`, 0013 table and presentation convention | Three-skin implementation status and old current UI guidance | Register the reviewed superseding decision and current BoardUI content owner. Keep E2E failure rules and Engine boundary. |
| `app/desktop/README.md`, package map, launch/demo and independent BoardUI sections | `internal/uikit`, `UNIC_UI_SKIN`, three-skin screenshots and “does not replace production” | Current package locations, supported launch options, unified visual scheme and accurate acceptance commands/boundaries. |
| `app/desktop/UI-DEMO.md`, `QUICK-PANEL-DEMO.md`, related active guides and fixtures | Skin-selection commands and old component/geometry paths | New actual commands and source maps. Historical screenshot receipts remain historical. |
| Architecture frontmatter `paths` | Deleted `internal/uikit` paths, including `desktop-runtime`'s `split.go` reference | Track the actual new code owners and run docscheck/bump only after verifying them. |

Use the repository decision lifecycle for the superseding choice. Do not reserve an ADR number from this document without re-reading the live ledger. At closure, merge implemented facts into architecture/contracts and update verification stamps. The execution receipt must still identify pending or unexecuted platform work.

## Execution receipt

The machine-readable [acceptance receipt](boardui-mygo/production-acceptance.json) binds 31 unique software-view cases and four process companions to their named manifests and checksums. All 35 have passing receipts. The broad raw run remains FAIL: six cases were diagnosed and have explicit passing replacements. This is aggregate acceptance, not a claim that the original broad run passed unchanged. The final composition covers 58 complete-view scenarios. The current production witness is `wrapper-accepted`; the current public control/reference witnesses are `reference-accepted`.

The final official React comparison passes 13 paired scenes with the 0.5 DIP control-geometry and 2/255 empty-interior color gates. Unmasked raster metrics and original/pair/overlay/difference images remain in `.build/boardui-migration/reference-accepted/visual`. These are component checkpoints, not a full-product official-template comparison. Source identity matches the current public package. The receipt explicitly identifies the two late runtime corrections and their focused replacement evidence; other case snapshots remain bound to their recorded source hashes.

The full Go race suite, zero-issue lint, Engine dependency boundary and darwin/arm64, linux/amd64 and windows/amd64 builds passed. A local unsigned macOS archive also passed executable/private-service smoke and byte-exact BoardUI/Inter/Remix license inclusion. No installer, GUI, signing or publication ran. The sampled race-enabled software-view test processes peaked at 162,562,048 bytes RSS for settings and 191,152,128 bytes for states. These include test overhead and are not production or Windows memory measurements.

Replay once into a new directory:

```sh
python3 app/desktop/e2e/replay_boardui_migration.py --out .build/boardui-production-review
```

Run the independent native demonstration with `go run ./app/desktop/cmd/boardui-demo --theme light`. This starts a new GUI only when explicitly executed. The same production content is exercised with synthetic fixtures by the canonical software cases.

Owned downloaded upstream source, reference dependencies/cache, package archives and scratch executables were removed. First-failure evidence, small receipts and screenshots remain ignored locally. Native macOS/Linux/Windows GUI, OS IME/accessibility, Windows native memory, cross-application paste and user visual approval remain NOT_RUN. Select typeahead, Slider range/RTL and a pinned CJK fallback remain documented limits. Catalogue coverage is explicit; this delivery does not claim all 69 components.

## Application icon conversion failure inventory

Before replacing the application glyph implementation: an icon name can map to a nonexistent or semantically wrong Remix export; a generated SVG can omit its complete viewBox/fill/path; an old hand-drawn or skin-specific glyph can survive; a helper-only glyph can be omitted; an unknown glyph can create unlimited cache entries; currentColor can stop following its parent; a raw SVG or dependency change can lose provenance; the old Lucide-specific notice can be mistaken for the new asset license; or renderer parsing can fail after successful source generation.

The conversion must inspect the exact `@remixicon/react` 4.9.0 module from the locked reference dependency tree, retain all actually used public/helper glyph names, generate complete canonical SVG strings and a small source/hash manifest, remove platform stroke/glyph overrides, and bound the parsed cache by that finite generated dictionary. The application semantic icon vocabulary is not a new standalone icon collection. Rendered production E2E remains required after generation; a manifest check alone is not visual acceptance.

### Icon source-generation receipt

The application glyph conversion generated 49 complete SVGs from the locked `@remixicon/react` 4.9.0 exports. `desktopkit/icons-source.json` records npm integrity, source-module/lock/generator hashes, glyph export/hash pairs and the retained Remix notice. `desktopkit/scripts/gen_icons.mjs` regenerates the data from the isolated reference dependency tree. The unchanged semantic IconName vocabulary maps to one cross-platform glyph set. A `sync.Once` cache parses at most those 49 entries; OS/theme/widget IDs do not create new keys. The public BoardUI package's original six-glyph set is independent and unchanged.

The expected initial provenance audit found all 49 former canonical SVG renderings absent; its receipt remains in `.build/boardui-icons/initial-red.json`. The subsequent source audit passed with zero missing canonical SVGs, and every generated SVG SHA256 matched the small manifest. This is source provenance evidence, not rendered/native icon acceptance. Complete production E2E and build results belong to the migration owner's final receipt.

## Implemented integration

The runtime has one public native BoardUI control package and one internal
application-composition package. All former UI Kit imports now target the
application compositions, which delegate Button, Input, Select, confirmation,
Switch, Checkbox, Slider and Segmented controls to the public implementation.
No raw platform token table, skin ID, environment-driven appearance selector,
legacy compatibility import or platform glyph override remains. Quick Panel's
separate GPUI color constants were replaced by the same semantic resources.

The application-specific list/grid/table virtualization, search token model,
settings/service binding, safe device confirmation, cancellation and host
process boundaries remain. These compositions are not additional official
BoardUI components. Context and editing menus use mygo's maintained in-window
menu adapter with the BoardUI theme; tray/Dock menus stay native host menus.
A hidden host title bar reserves the native caption before application content.

The resource lock now includes 111 colors per theme, the added selection-control
source identities and 2xs/checkbox-selected shadows. The application icon set
contains 49 exact Remix 4.9.0 SVGs with a finite cache. No module dependency or
Engine/service protocol changed. Source fonts, notices, registry source and
reference compiler identities retain their individual hashes and licenses.

## Diagnosed migration failures

Initial source/theme red: `red-01` and
`.build/boardui-production-migration-initial-red` retained old imports, selectors,
font/theme mismatches and complete synthetic page screenshots.

`composition-01` exposed an obsolete segmented test expectation: the pinned
React Aria toggle group uses arrows for focus and Enter/Space for selection.
The revised complete-view scenario retains both focus and value assertions.
The later `all-view-final` composition subcase still waited only 250ms for a
source 300ms Sheet exit. It was corrected to wait through the source duration;
`composition-final` passed all 58 retained scenarios without a runtime change.

`replay-01` menu failures came from duplicate background labels (`Today` and a
sidebar device). The corrected test clicks the actual visible option role and
bounds. `menus-role-target-01` passed all 12 theme/action combinations. The
first tag-manager diagnosis incorrectly treated `Tester.Menu()` as system-only;
the fixed dependency documents that it also reports in-window menus. Acceptance
now requires actual visible menu roles, retains pointer dispatch and preserves
all value/persistence assertions. Table operations locate their real table cells
and scroll the virtual table to the logical first row before testing sort order.

The broad initial view run exposed actual retained-layout gaps: duplicate
DetailHeader/toolbar actions, a fixed 260-DIP parent clamp blocking search
expansion, lost search disclosure, and instant Sheet removal. The runtime fixes
keep commands in the toolbar, preserve responsive disclosure, and retain a live
modal body until source exit finishes. Table layout caches now read LayoutBounds
rather than transformed paint bounds, so source scale cannot shrink cached
column widths. Retained tests distinguish centered source scale/fade from the
retired top-slide/opaque material behavior; selection, logical column widths,
exit cleanup, resize, reversal and reduced-motion checks remain.

The first source-shadow extraction rejected a compiled OKLCH color. Its failure
is retained in `token-extraction-03`; the parser was extended to the actual CSS
color syntax and the same owned browser space was recovered successfully.

The first full Go test run used macOS's symlinked `/var` TMPDIR, which the
existing secure file cache rejects. A long canonical worktree TMPDIR then
exceeded the Unix socket pathname limit. Both failures are retained. The single
subsequent run used an owned short real `/private/tmp` path and passed the full
race suite. That directory was removed. No core implementation or security
contract was changed to bypass either check.

A final read-only audit found an application AlertDialog guard that used the
previous busy flag to reject same-update completion closure. Before fixing it,
`busy-completion-red` reproduced the reopened alert in both themes through the
complete widget view. The guard now also requires the current busy flag. The
full composition case is rerun after this narrow runtime correction. Current
production confirmation calls do not set BusyLabel; the correction preserves
the reusable application helper's documented asynchronous contract.

Final view runs also retained obsolete 250ms Sheet waits in composition/tag
scenarios. Those expected the retired skin duration and were changed to wait
through the locked 300ms source exit, keeping input-blocking and cleanup checks.
The 360-DIP virtual table can show part of its final 48-DIP row; acceptance now
checks visible intersection and fixed horizontal bounds, rather than requiring
whole-row scroll alignment. Sort, row-height limits and protected tag behavior
remain mandatory.

The typography extraction audit found 35 source styles in each theme whose
unused Tailwind variables were absent from compiled CSS. The old probe had
inherited browser defaults for those records. Before changing the extractor,
`typography-red.json` retained all 70 mismatches. The corrected probe parses the
four declarations from the pinned typography.css with PostCSS, injects only
those exact variables into a hidden probe, and asks the browser to resolve the
font metrics. Missing/duplicate declarations and invalid numeric results fail.
CSS computed `letter-spacing: normal` is explicitly the zero-spacing value.
The failed normal-spacing parse and recovery reuse the same owned TaskSpace.
The corrected extraction matches all 104 source scheme/style records. All 11
styles used by current production code retain identical size/line-height/weight/
tracking values; this corrects the additional public resource records without
changing those production styles. The source lock and final reference replay
are regenerated after this correction.

Two remaining motion assertions had used stale world coordinates: grid mode
inserts its size slider before the segmented group, and the sidebar toggle
moves with its content edge during collapse. Acceptance now reads the current
group-local sample and tracks the actual pointer target between press/release.
It retains real pointer events, selection/reversal state and final geometry;
it does not invoke a command or force a DOM action behind the native view.

Distribution audit used the existing native development-package constructor.
The actual initial application archive and stage manifest omitted all three
BoardUI/Inter/Remix notices; `notices-red` retains the failure. The stager now
copies the original notice bytes into its shared Resources/payload path before
hashing. `notices-green` rebuilt the real unsigned macOS application archive,
passed executable identity/private CLI-service smoke, and matched all three
archive/stage/source SHA256 values. This is local archive construction only;
GUI, installation, signing and publication remain NOT_RUN. Large scratch
archives/binaries are removed after keeping the small receipts.
