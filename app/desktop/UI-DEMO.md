# Native desktop layout demo

## Reference and scope

Reference: local `main` commit `8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7`.
The fixture is a page composition port to the migrated UI Kit. The normal desktop now reuses the composition with live service data.
The normal desktop entry still reads the daemon.
The separate `unic-desktop --demo` interface uses 22 history entries and four devices.
All demo operations use memory. Restart the process to restore the fixtures.
Copy and send commands show simulated results. They do not write the system clipboard or send data.
Settings controls retain their values during the session. They do not configure the application or system.
Only the appearance mode changes the rendered theme. Other appearance and behavior settings are fixtures.
Setup, space creation, unlock, startup-preview and recovery-preview pages have been removed. Both entries open history directly by default.

| Reference under `apps/gui-go/frontend/src/` at `main` | Native composition under `internal/desktopdemo/` |
| --- | --- |
| `layouts/LibraryMainLayout.tsx`, `pages/history-kit/HistoryKitPage.tsx` | `history.go`: sidebar, toolbar, list/grid, detail, filters |
| `pages/history-kit/HistoryKitList.tsx`, `HistoryKitGrid.tsx` | `history.go`: day groups, type icons, pins, tags, selection |
| `pages/history-kit/HistoryKitDetail.tsx` | `history.go`: text, code, link, image, files, metadata, action bar |
| `pages/history-kit/HistoryKitTagManager.tsx`, `HistoryKitTagNameCell.tsx` | `tag_manager.go`: tag table, editing, sidebar visibility, native context menus |
| `pages/devices-kit/DevicesKitSheet.tsx` | `devices.go`: management sheet, master/detail, invite/connect/phone tasks, pending fingerprint review, sync policy, latency |
| `pages/settings-kit/SettingsKitPage.tsx`, `components/setting/settings-config.ts` | `settings.go`: category sidebar and nine setting pages |
| `apps/quick-panel` and `crates/quick-panel-core` at cached `origin/main` `885546c7` | `quick_panel_*.go`: isolated GPUI quick panel fixture; see [acceptance guide](QUICK-PANEL-DEMO.md) |
| `updater/app-icon.png` | `app-icon.png`: original embedded app icon |

The Kit selects Adwaita, macOS glass, or Fluent and applies the matching tokens.
The demo reuses the Kit's existing native approximations for window chrome, controls, materials, and animations.
The dedicated `--page quick-panel` entry uses a compact frameless window and a nonfocusable satellite. It simulates host effects and retains a separate GPUI source reference. See the [quick panel guide](QUICK-PANEL-DEMO.md) for interaction coverage and native acceptance limits.
The demo supports fixture search operators and in-memory tag-library editing. It omits daemon integration, network recovery, and real file operations.
It does not claim pixel parity with the reference or native acceptance on any platform.

## Build and open

From the repository root:

```sh
go build -o .build/unic-desktop ./app/desktop/cmd/unic-desktop
UNIC_UI_SKIN=macos .build/unic-desktop --demo --theme light
```

Run one command at a time. Quit the demo before opening the next skin:

```sh
UNIC_UI_SKIN=linux .build/unic-desktop --demo --theme dark
UNIC_UI_SKIN=windows .build/unic-desktop --demo --theme light --page devices
UNIC_UI_SKIN=macos .build/unic-desktop --demo --theme dark --page settings
```

Initial pages: `history`, `devices`, `settings`, `quick-panel`.
Color modes: `system`, `light`, `dark`.
The History toolbar's More menu exposes the quick-panel fixture. Setup and unlock routes are rejected.
Settings > Quick Panel opens the quick panel composition.

## Manual end-to-end acceptance

The user owns visual interaction acceptance. Do not launch the application or an offscreen renderer without an explicit request.
Repeat the following sequence for each skin and color mode.
Use the same binary and reset the fixtures by restarting the process.

1. Open the history page. Confirm the library, tags, devices, grouped list, detail, and action bar are visible.
2. Select the code, link, image, and file entries. Confirm each preview matches its content type. Open and close image Quick Look.
3. Search for `产品`. Confirm one result. Search for `no-match`. Confirm the empty state. Clear the filters.
4. Filter by device, tag, type, and today. Confirm every visible entry matches the selected filters.
5. Switch to grid. Change card size. Select a card. Confirm the inspector opens.
6. Select multiple items. Confirm the selection summary. Pin the selection. Request deletion. Cancel it. Request deletion again and confirm it.
7. Open Manage Devices. Confirm the sidebar shows pending requests, the local device, linked devices (online first, with latency) and suggested devices. Select each device. Confirm the latency, link and last activity. Set the direction to Pause. Confirm the content switches are disabled and the summary changes. Turn off global sync in Settings. Confirm the notice appears.
8. Open Add > Invite Device. Confirm the 6-digit code, the regenerate action and the ticket mode. Simulate a join. Confirm a pending request appears and its detail shows the fingerprint with Reject and Accept. Accept it. Confirm the device appears. Repeat and choose Reject.
9. Open Add > Connect Device. Confirm Connect stays disabled for unknown input and the hint names a code or a ticket. Enter `000-000`. Confirm the error. Enter `482-913`. Simulate approval and rejection. Open Add > Connect Phone and confirm the code step.
9a. Request device removal. Confirm the dialog states three consequences. Cancel it. Confirm it. Confirm the local device cannot be removed. Rename a device and confirm the sidebar updates.
10. Open all nine settings categories. Change color mode. Confirm it updates the view. Return to History.
11. Resize to 1000 px. Open the sidebar as an overlay. Close it. Resize to 700 px. Select history. Use Back to return to the list. Open Manage Devices at 600 px. Confirm only the device list shows. Select a device. Confirm its detail fills the sheet. Use Back to return to the list.
12. Open the quick-panel fixture. Confirm selection and preview.
13. Confirm `--page setup` and `--page unlock` fail before mygo starts.

## Acceptance artifact

At the end of a manual run, retain a receipt and screenshots in a chosen artifact directory.
Record the host OS, binary SHA-256, source HEAD, dirty source diff, skin, color mode, viewport, scenario, observed result, and screenshot filename.
Include wide history, grid, image detail, device sheet, settings, quick panel, and narrow navigation screenshots.
Record PASS or FAIL for each step. Mark untouched scenarios NOT RUN.
Repeat the same commands and sequence to reproduce a result.
Cross-compilation and static analysis are build evidence only. They cannot replace this artifact.

## Reference-image geometry

The supplied reference image is 2200 x 1440 physical pixels.
Its layout is consistent with the 1100 x 720 default demo window at 2x scale.
The dimensions below come from the reference main skin CSS. They are logical pixels, not physical pixels.

| Region | Reference geometry | Previous mismatch | Correction |
| --- | --- | --- | --- |
| Window | 1100 x 720, full-height content | 1180 x 760 and separate native title bar | Hidden native title bar, retained native traffic lights |
| Columns | 236 / 500 / 364 | Wrong window aspect ratio | Use the reference default size and Kit column widths |
| Sidebar | 52 top, 10 horizontal, 12 bottom | Additional 8 px nav padding | Remove nested padding; keep top controls in the sidebar band |
| Sidebar groups | 28 px rows, 28 px headings, 10 px group spacing | Uniform compact spacing, square tag swatches | Restore group rhythm and round tag dots |
| Toolbar | 52 px band, 32 px capsules | 24 px inset segments and loose commands | Toolbar-specific glass segmented control and grouped commands |
| Search | 300 x 32 | 260 px slot and plain field styling | Restore width, hint and glass capsule material |
| List | 48 px rows, 28 px day headers | Bare glyphs, missing counts and sparse data | KindBadge/thumbnail, day counts, 8 today + 14 yesterday |
| Detail | 24 px horizontal gutter; 12 px preview radius | Wrong metadata mark and extra caption | KindPill, metadata, original preview structure |
| Delivery | 34 px inset rows | Large settings cards | Use InsetList and device rows |
| Action bar | 60 px height, 30 px radius, 16 px edge inset | 64 px docked row flush to pane edges | Float the bar over the detail; reserve scrolling clearance |
| Main action | 40 px primary, 32 px secondary | 32 px primary and small rectangular secondary | Restore sizes and secondary glass capsule |

The shared Kit now provides the macOS toolbar context styling that was missing from the initial port.
Native blur and font rendering can still differ from the reference web skin.
No current screenshot has been captured or compared pixel by pixel.
Build success must not be reported as visual parity.

### Toolbar segment collapse correction

The macOS toolbar segments require an explicit 34 px flex basis and minimum width, with shrinking disabled.
`Grow(1)` installs a zero flex basis in mygo. A later `Grow(0)` retains that basis, so width alone cannot preserve the segment geometry.
The correction restores the basis before disabling growth.
This change has package build and static-analysis evidence. Native screenshot acceptance remains pending.

### Sidebar content clipping correction

At the default 236 px macOS sidebar width, its 10 px horizontal surface padding leaves 216 px for navigation content.
The fixed-width inner sidebar is 236 px including its padding, leaving 216 px for navigation. The animated outer viewport has no padding and can reach zero width.
The More row follows the reference 36 px leading inset, 24 px height, and secondary text color.
Neutral macOS device status uses label-3; fg3 is not a macOS token.
The sidebar heading uses the final reference CSS override: 28 px height and 10 px group spacing.

### Sidebar motion correction

A user-supplied 4.28-second recording shows the sidebar closing at approximately 2.25 seconds.
The toggle and toolbar inset previously read the hidden boolean directly while sidebar width animated independently.
This made the toggle jump before the sidebar finished closing and made the toolbar title jump sideways.
The macOS composition now derives toggle position, toolbar inset, and sidebar-content translation from the same animated width.
The transition matches main: 280 ms, cubic-bezier(0.2, 0, 0, 1). Mygo applies the system Reduce Motion preference.
The sidebar content keeps its natural width and slides out instead of remaining stationary under a shrinking clip.
The reference recording establishes the previous behavior. No post-fix UI has been launched or recorded.

### History scrollbar placement and visibility

The history list extends its scroll viewport 8 px farther toward the pane edge.
List and grid content reserve an additional 12 px right gutter so native scroll thumbs do not cover tags or cards.
The Kit tracks actual scrolling with mygo TrackScroll, attached before List.Rows.
Position changes show the native indicator. After 600 ms without movement, it hides even if the pointer remains over the list.
Hover alone does not show it. The scheduled idle frame hides it without continuous redraws or a background goroutine.
Wheel scrolling, keyboard reveal and visible-thumb interaction remain owned by mygo.
For acceptance: hover without scrolling; scroll; stop with the pointer still over the list; wait 600 ms; scroll again; then use keyboard navigation past the viewport and switch to grid.
Confirm the indicator stays outside content and follows the same visibility rule in both layouts.
The result still requires native interaction acceptance.

### Sidebar padding-floor regression

The prior motion correction synchronized control positions but left 20 px of horizontal padding on the animated sidebar surface.
Mygo clamps an element width to at least its padding. The sidebar therefore stayed at 20 px during the closing tail and jumped to zero when removed.
The corrected hierarchy is a padding-free animated viewport around a fixed 236 px inner body. The inner body owns all sidebar padding.
The viewport remains mounted at zero width. Navigation state survives closing and reopening.

`TestSidebarCollapseE2E` drives the complete demo view through the existing UI event/layout engine without opening a native window.
It checks the closing tail, reopening, and reversing an in-progress transition.
It writes repeatable geometry data, not visual acceptance screenshots:

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/sidebar-after" \
  go test ./app/desktop/internal/desktopdemo -run '^TestSidebarCollapseE2E$' -count=1 -v
```

Before correction, the content x coordinate at 200/240/260/280 ms was 20/20/20/0 px.
After correction, it was approximately 7.77/2.42/0.96/0.18 px.
The original implementation fails this scenario. The corrected implementation passes.
These measurements establish layout continuity in the UI engine. Native appearance and interaction remain user-owned acceptance.

### Shared scroll-container ownership

`Kit.Scroll` is the entry point for standard vertical scrolling in desktop pages and Kit components.
Its visibility behavior lives only in `internal/uikit/scroll_indicator.go`. Virtual lists and grids use the same policy function.
Desktop code must not construct `ui.Scroll` directly outside that factory.
Detail, nested content preview, settings, sheets, device master/detail, transfer details, sidebar navigation, and the daemon-backed GUI now use the shared constructor.
Each scroll container keeps its own position and idle timer. Scrolling a nested preview must not display the idle parent's indicator.
No page-specific timer, hover flag, or scrollbar option is required.

The full-view E2E scenario covers history list, history grid, outer detail, and nested preview in three skins and two color modes (24 cases).
For each case, it checks the native indicator's edge pixels before interaction, on hover, during actual scroll input, and after 800 ms with the pointer still inside.
Preview cases also check that the parent indicator remains hidden.
The baseline failed all 12 detail/preview cases. The migrated implementation passes all 24 cases.
The output contains measurements, not screenshots or native acceptance claims:

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/scroll-policy-after" \
  go test ./app/desktop/internal/desktopdemo -run '^TestUnifiedScrollbarsE2E$' -count=1 -v
```

### Liquid Glass material

The macOS toolbar capsules, toolbar view switch, search field, floating action bar, and secondary Glass buttons share `Kit.glassSurface`.
That helper now uses the existing mygo `plugins/glass.Glass` material with Regular style and Interactive enabled.
The previous implementation only blurred the background and painted token-based shadows and rims.
The plugin supplies the lens/refraction shader, reactive rim lighting, press growth, and Reduce Transparency / High Contrast fallbacks.
No new dependency or page-specific effect implementation was added.
The search field retains its focus ring and no longer adds the old field shadows over the material.

The native Go UI is painted into one render surface. The plugin's shader samples that already-painted UI.
Its optional Native mode puts NSGlassEffectView below that surface and samples a WebView or window material, not the Go UI painted above it.
The demo therefore uses the maintained shader path for these native Go controls. It does not claim direct NSGlassEffectView composition.

`TestLiquidGlassE2E` drives the complete demo in light and dark mode at 2x scale.
It samples the rounded upper rim on hover and after exit, including the corners where light-mode highlights remain measurable.
For the secondary Glass action, it also checks native menu tracking on press. The OS owns the pointer while that menu is open.
A read-only Go source overlay replaying the legacy blur helper fails all six scenarios with no reactive rim or press growth.
The current material retains reactive hover lighting and resets after pointer exit. The send action now verifies platform menu tracking rather than requiring button press growth while the OS owns the pointer.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/glass-regression" \
  go test ./app/desktop/internal/desktopdemo -run '^TestLiquidGlassE2E$' -count=1 -v
```

The software-renderer result is recorded in liquid-glass-feedback.json. Native Metal appearance and frame-time behavior have not been accepted.

### Interaction motion

The 20:54:18 recording exposed static control content, a teleporting selection thumb, and immediate menu presentation.
The PixPin click halo is a recording overlay and is excluded from application feedback.
The shared Kit applies 80 ms press compression and a 350 ms spring return to macOS buttons and segments.
Selected toggles do not stay compressed. Glass buttons retain their existing material expansion.
The toolbar selection uses one moving thumb, including when a new selection interrupts an unfinished spring.
System menus own their presentation and pointer tracking; they do not use Kit scale/fade animations.
Pointer selection no longer paints a segment's keyboard focus ring. Text-input focus remains visible.
Reduce Motion suppresses the spatial effects through the existing renderer preference.

`TestInteractionMotionE2E` checks press/release geometry, press cancellation, rapid selection reversal, and Reduce Motion.
Its menu check asserts delegation to the system, rather than an application-painted popup transition.
It records actual view geometry and rendered selection pixels in `interaction-motion.json`.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/interaction-regression" \
  go test ./app/desktop/internal/desktopdemo \
  -run '^(TestInteractionMotionE2E|TestLiquidGlassE2E|TestUnifiedScrollbarsE2E|TestSidebarCollapseE2E)$' -count=1 -v
```

Manual acceptance: alternate list/grid quickly, hold and release a toolbar button, drag outside to cancel, and open the overflow menu.
Check both color modes and Reduce Motion. Native Metal smoothness and subjective interaction feel still require user acceptance.

### macOS dropdowns and menus

Select and uncontrolled Menu/ContextMenu use mygo's system menu API on the macOS skin.
Kit.Apply installs SystemMenus. The current mygo Darwin backend constructs NSMenu and invokes popUpMenuPositioningItem:atLocation:inView:.
The OS owns the menu's material, spacing, highlight, pointer tracking, dismissal, and animation.
The Kit does not place a custom glass surface or spring transition around a system menu.
This applies to all four history filters, send menus, overflow menus, settings selects, and compact navigation menus through the shared component implementations.
An explicit Open pointer retains the existing caller-controlled in-window overlay contract.
Menu entries reuse the list-menu adapter. Template glyph images follow the system highlight; tag swatches retain their color.
Select preserves its value and Changed result. Pointer focus no longer leaves a blue keyboard ring around the filter pill.
Native menu triggers align to the center of their parent row. TestToolbarAlignmentE2E checks both toolbar menu triggers against the adjacent pin button in light and dark mode. The former top alignment placed both triggers 3 DIP above the pin button. Geometry is recorded in toolbar-alignment.json.

`TestNativeMenusE2E` drives the complete history view in two color modes.
The baseline failed all 12 cases because it never requested a system menu on pointer-down.
The current checks cover opening on press, cancelling without changing state, selecting an item, closing, and returning the selected value or command.
The tester captures the platform menu request. It does not draw NSMenu or establish native animation acceptance.
Measurements are written to native-menus.json:

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/native-menus-regression" \
  go test ./app/desktop/internal/desktopdemo -run '^TestNativeMenusE2E$' -count=1 -v
```

Manual acceptance: restart the demo; open each filter, send menu, and overflow menu; move across rows while holding the pointer; release on a row; cancel with Escape or an outside click.
Check menu appearance and animation in the actual macOS window. Native acceptance remains separate from the platform-request replay.

### Toolbar search disclosure

The 21:35:53 Finder recording is the motion reference for the search icon and search field only.
The existing title bar, traffic-light placement, sidebar toggle, view switch, and command capsule remain in place.
The demo does not install NSToolbar or NSSearchToolbarItem.

Kit.TokenSearchField in toolbar mode shows a 32-point search icon on macOS.
Pointer activation or Cmd+F expands the existing glass capsule to 460 points over 280 ms and focuses the input.
The right edge of search and the adjacent command capsule stay fixed. The toolbar's existing flexible space accommodates the changing search width.
A fixed-width input body is clipped by the animated viewport, so text does not reflow during expansion or collapse.
Escape closes candidates first, then clears text and conditions, then collapses an empty search. Leaving an empty search also collapses it.
A nonempty query remains visible after focus leaves. The clear button clears the query and retains input focus.
The expanded empty field retains the original placeholder and shortcut hint: "搜索，或输入 # @ / on:" and "⌘F".
Reduce Motion follows the existing renderer preference. Other skins use their regular token search field.

TestSearchDisclosureE2E drives the complete history view with actual pointer, keyboard, typing, and layout events.
The baseline failed because search was always 300 points wide and had no icon trigger.
The check records intermediate widths during expansion and collapse, verifies unchanged command geometry, types a query, verifies actual filtered results, and checks Escape, Cmd+F, and outside-click collapse.
It writes search-disclosure.json. The event/layout checks do not establish native Metal visual acceptance.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/search-disclosure-regression" \
  go test ./app/desktop/internal/desktopdemo -run '^TestSearchDisclosureE2E$' -count=1 -v
```

Manual acceptance: restart the demo; activate the search icon; inspect expansion and collapse; type and clear; move focus outside an empty field; verify the original title bar and right command capsule.

### Demo condition search

The history demo now adapts the original main composite-search behavior to the existing Go TokenSearchField. The independent model is desktopdemo/search_model.go; the Kit only owns presentation, focus, selection plumbing, and native editor key dispatch.

| Prefix | Candidates | Example |
| --- | --- | --- |
| @ | Fixture device IDs and displayed names | @Linux |
| # | Existing fixture tags; multiple selections intersect | #设计 |
| / | text, richtext, image, file and localized labels | /image |
| on: | today, yesterday, last_7d, last_30d, this_week, this_month | on:yesterday |
| ext: | txt, md, jpg, png, pdf, ts, js, json, rs, go | ext:png |

Candidate counts are projected from the same fixture filters and remaining free text as the actual list. File-extension counts include names inside file bundles. Time presets use the fixture calendar (Monday, November 9), not the host clock. Rich text has zero fixture entries and remains a selectable, muted candidate.
The regular type, device, tag and time filters project into the same condition chips. Removing a chip resets its dimension. A multi-tag selection is represented by one tag chip; selecting a single tag from the existing tag dropdown replaces that selection.

Up/Down wrap through candidates; Enter or non-shift Tab completes a condition. A pointer choice retains input focus. Backspace in an empty input deletes the entire last condition badge, including all tags in a multi-tag badge. The remove button uses the same operation. Deletion resets that dimension, preserves the other dimensions, and never copies badge text back into the input.
Completion removes only the active condition word and preserves surrounding text and caret position. Completed exact words followed by whitespace can commit when pasted together. Quoted words, leading escapes, unknown prefixes and paths remain literal search. IME composition keys stay with mygo's editor.
Escape dismisses candidates, then clears text and filters, then releases/collapses the empty field. Clear-all retains focus. The original placeholder and shortcut hint remain when the field is empty without chips.

The source reference is main 8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7: HistoryKitSearchField, useCompositeSearchBar, composite-search-model, search-token-ranges, and the macOS token-search skin.
The former plain ToolbarSearchField helper was removed; the history demo uses the existing token component with a toolbar variant.

TestTokenSearchE2E drives fourteen scenarios in three skins and two color modes (84 cases). It covers all prefixes, counts under other conditions, tag intersection, pointer/keyboard completion, clear, filter/chip synchronization, literal and quoted input, pasted conditions, Unicode caret preservation, Backspace, and layered Escape. The first baseline failed all 42 token scenarios; the six literal cases already passed.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/token-search-regression" \
  go test ./app/desktop/internal/desktopdemo ./app/desktop/internal/uikit -count=1
```

The replay writes token-search.json and a build receipt records source/artifact hashes. Full view and Kit checks passed (544 reported results). This is event/layout/software-renderer evidence; native Metal appearance and input-method acceptance remain separate.
Restart the demo and try @Linux, #设计, /image, on:yesterday and ext:png. Try several exact words with trailing spaces, then remove or edit a chip. The daemon-backed GUI and the separate GPUI quick-panel fixture are outside this history-demo search integration.


### Tag management sheet

The history sidebar opens the management sheet through Manage Tags or its add control.
The sheet uses the existing Kit Sheet, Table, Select, SearchField, Switch, and platform menu adapter.
It provides most-used/name/newest sorting, name filtering, reference counts, sidebar visibility, and a footer summary.
The fixture registry contains the three existing custom tags, one unused custom tag, and four builtin tags.
The registry supplies the sidebar, search candidates, list badges, and detail badges.

Custom tags support inline creation and renaming, five colors, merging, and single or multiple deletion.
Empty names cancel editing. Duplicate names retain the editor and show an error. Escape cancels editing.
Deletion removes tag metadata and associations while preserving all history entries.
Merging unions associations into the target custom tag and removes the source tags.
Builtin tags expose only Show Entries in their context menu. Their sidebar visibility remains editable.
On macOS the table requests a real NSMenu; the OS owns its appearance and tracking.
The existing Add Tags detail action remains a separate assignment picker.
All operations remain in memory and reset on restart.

The macOS sort selector uses the same 32-point height, 16-point corner radius, and shared glass material as the adjacent search field. Its position and NSMenu behavior remain unchanged. Other Select instances keep their standard appearance.

The macOS table uses a 26-point header, approximately 32-point rows, alternating row backgrounds, and rounded selection.
Other skins retain their platform table density.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/tag-manager-regression" \
  go test ./app/desktop/internal/desktopdemo ./app/desktop/internal/uikit -count=1
```

TestTagManagerE2E covers fifteen scenarios in light and dark modes and writes tag-manager.json.
TestTagManagerPlatformsE2E covers the three skins in both modes and checks the separate assignment picker.
The initial management scenarios failed because the former sheet had no table.
A subsequent geometry check detected the oversized header and rows before the Kit table spacing correction.
These are complete-view event, accessibility, layout, and platform-menu-request checks.
Actual NSMenu rendering, native sheet appearance, and animation acceptance remain NOT RUN.

Manual acceptance: open Manage Tags; filter and sort; create and rename a custom tag; open its context menu; change its color; toggle sidebar visibility; merge or delete it; confirm history entries remain.
Right-click a builtin tag and confirm that only Show Entries appears.
Close the manager and use Add Tags in the detail pane to confirm assignment remains separate.

The workspace now exposes the desktop command at app/desktop/cmd/unic-desktop.
Build and open this entry after the service/desktop split:

```sh
go build -o .build/unic-desktop ./app/desktop/cmd/unic-desktop
UNIC_UI_SKIN=macos .build/unic-desktop --demo --theme dark
```


### Fixed columns in tag management

The tag management sheet sets TableProps.FixedColumns for its table.
This delegates to mygo's TableColumn.Fixed and disables both resizing and reordering.
Row selection, sidebar switches, and native context menus remain interactive.
Other tables retain their existing draggable-column behavior and width allocation.

TestTagFixedColumnsE2E drives the complete sheet across three skins and both color modes.
It attempts to shrink, grow, and reorder each column and checks that its geometry stays unchanged.
It also toggles a sidebar switch and invokes Show Entries from the row context menu.
The initial check failed on all six configurations because the name column still resized.
The replay writes fifty-four column measurements to tag-fixed-columns.json.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/tag-fixed-after" \
  go test ./app/desktop/internal/desktopdemo -run '^TestTagFixedColumnsE2E$' -count=1
```


### macOS sheet motion

The shared Kit Sheet now slides down from the top over 300 ms and retracts upward over 220 ms.
A separate stationary scrim fades over 220 ms.
DialogBase retains event scoping, dismissal, focus trapping, and opener focus restoration.
The original live dialog remains mounted through exit. It retains its modal scope until motion completes, then returns focus to the opener.
Exit does not use a copied element tree or switch rendering paths. Table columns, selection, and opaque surface colors stay unchanged while the panel moves.
The implementation uses existing mygo animation and transition APIs. It does not introduce an animation dependency or an NSWindow sheet bridge.

Keep calling Sheet while Open is false so the live dialog can finish its exit motion.
Open records the requested state. OnClosed runs once after exit completes; clear transient selection, editing, and device-task state in that callback.
The demo now follows this lifecycle for tag management, device management, and tag assignment.
Reduce Motion removes the panel's translation; the scrim may still fade.
Adwaita and Fluent retain their existing DialogBase behavior.

TestSheetMotionE2E checks three sheets, two color modes, three dismissal paths, and both motion preferences (36 scenarios).
It records an intermediate entry position, the resting position, and sampled software-renderer pixel differences during exit in sheet-motion.json.
The initial checks found immediate placement at Y=0 with no entry animation.
The regression also checks that completed exits release input and that the existing Kit focus test returns focus to the opener after exit.
Existing tag-management interaction tests now advance the deterministic test clock until entry completes before interacting.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/sheet-motion-regression" \
  go test ./app/desktop/internal/desktopdemo ./app/desktop/internal/uikit -count=1
```

Manual acceptance: restart the macOS demo; open and close each sheet using Done, Escape, and the backdrop; check smooth motion, a stationary fading scrim, opener focus, and right-click menus. Repeat with Reduce Motion enabled.
Native Metal appearance and motion acceptance remain NOT RUN.


### Exit frame stability

The user recording PixPin_2026-11-09_23-54-32.mp4 shows compressed table columns and a brightness jump as exit starts.
The former retained-tree exit path rebuilt layout for a virtual table. Sheet state was also cleared as soon as Open became false.
The current lifecycle keeps rendering the original dialog at a translated position until the 220 ms exit ends.
The scrim fades separately through its background alpha. The panel keeps its opacity and rendering path.
Tag selection and editing state, and device-task state, are cleared by OnClosed after the panel leaves.

TestSheetExitStabilityE2E selects a tag and closes the full management sheet through Done, Escape, and the backdrop in both color modes.
At 16, 32, and 48 ms it checks identical column widths, unchanged selection, and identical opaque-gutter RGB values.
It also checks that the panel and selection are removed after exit completes.
The former lifecycle failed all six scenarios because the live table had already been removed.
The replay writes sheet-exit-stability.json alongside the complete regression artifacts.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/sheet-exit-stability-regression" \
  go test ./app/desktop/internal/desktopdemo ./app/desktop/internal/uikit -count=1
```

The recording was inspected as contact sheets and a 60 fps exit sequence without launching the application.
The new checks use the software renderer. A new native recording is still required to confirm Metal material continuity.


### Send target menu

The detail action bar's Send To button now opens the existing Kit Menu with its large Glass button style.
On macOS this requests NSMenu through the existing system-menu adapter.
The first item is All Devices, followed by a separator and every remote device in the current fixture registry.
The local device is excluded. Offline peers remain listed. The All Devices item is disabled when there are no remote devices.
The toolbar send menu and detail send menu share this projection and command handling.
Renamed or removed fixture devices appear immediately in both menus.
Opening or cancelling the menu does not send. Selecting a target shows a simulated result naming that target.
This demo does not perform real network transfers.

TestSendDeviceMenuE2E covers six cases in each color mode: all devices, each of the three peers, cancellation, and changed device names/membership.
It verifies a platform menu request, exclusion of the local device, and the returned target.
The initial checks failed because the detail action was an immediate-send button with no menu.
The replay writes send-device-menu.json. Actual NSMenu appearance and placement require native acceptance.

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/ui-demo/send-menu-regression" \
  go test ./app/desktop/internal/desktopdemo ./app/desktop/internal/uikit -count=1
```

## Live service binding

The normal entry uses `NewDesktop` in `live.go` and the existing composition.
It does not initialize fixture history, devices, tags, latency or clipboard effects.
History uses service pagination and server-side type/device/tag/time filters.
Refreshing preserves the loaded page depth. Counts describe loaded results; a trailing `+` indicates more pages.
Selected text, PNG and file-name previews use bounded content readers. PNG previews accept at most 32 MiB and 16 million pixels; text previews accept at most 1 MiB.
Restore, tag replacement and deletion use the shared local service client.
Mutations never retry automatically after uncertain responses.
The device sheet binds ticket/code pairing, candidate requests, fingerprint/revision approval, rejection and removal.
General, Sync and Storage settings bind existing configuration. Native login status comes from the launcher.
Full-text/extension search, favorites, full tag management, device policies, history-to-device send and quick-panel OS integration remain capability gaps.
Unavailable operations do not produce simulated success in the normal entry.
See the [execution receipt](../../docs/exec/desktop-live-data.md) for process checks and native limits.
