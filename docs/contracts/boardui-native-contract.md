# Native BoardUI component contract

Status: native controls and production presentation integration. Executed acceptance and pending platform work are recorded in the [component receipt](../exec/boardui-mygo-implementation.md) and [production migration receipt](../exec/boardui-production-migration.md). This contract does not change Engine, plugin, sync or local-service protocols.

## Scope and ownership

The reusable public Go package is `github.com/uniclipboard/unic/app/desktop/boardui`. It depends on mygo and Go standard-library resources. It owns public control paint, source-bound resources and native primitive composition. `app/desktop/internal/desktopkit` owns application-only compositions and adapters. `desktopui` and the production command retain domain models, service binding, validation, persistence and async work.

Main GUI, setup, Quick Panel, the full Desktop demo and the isolated component demo share one BoardUI content presentation on macOS, Windows and Linux. The former `internal/uikit` package, platform token snapshots and three-skin selector are removed. There is no legacy-skin fallback. System/light/dark are color-scheme choices rather than platform design systems.

Core Engine and UI-free CLI/service programs must not acquire either UI dependency. The public control package does not own windows, the clipboard, OS input delivery, networking or persisted application settings. The internal application composition must not become an additional public UI library or an independent theme source.

## Source and license identity

The sole BoardUI source reference is official MIT export `ec5c5eb3e29c2441be63b9422cd54f411ef23466`. Actual source takes precedence over stale comments or newer skill recipes. Source-derived code/resources retain the BoardUI MIT notice. Embedded Inter 4.1 retains OFL 1.1. Functional Remix assets retain their independent license.

The [public source lock](../../app/desktop/boardui/assets/source-lock.json) binds source files, exact reference compiler dependencies, font, generated resource counts and helper glyph hashes. The current generated resource set contains 111 semantic/accent colors per scheme, 52 complete typography styles and source geometry/motion. The [application icon lock](../../app/desktop/internal/desktopkit/icons-source.json) separately binds its finite 51-glyph Remix 4.9.0 vocabulary. The public helper-glyph set remains separate. Neither cache uses transient widget/row IDs.

The repository root license remains AGPL-3.0. Upstream MIT, OFL and Remix notices retain their independent scope; this work does not relicense existing repository code. No Pro implementation or source-unverified newer component is implied by catalogue metadata. Required new resources must be inspected/generated from lawful fixed source rather than silently approximated or fetched from restricted endpoints.

## State and lifecycle

`Kit{Dark, Colors}` selects immutable theme resources and does not own widget or domain state. A nil `Colors` preserves the source presentation. `NewPaletteTheme(light, dark)` accepts up to twelve base colors per scheme and derives component states with mygo Oklab mixing. Zero base colors inherit the scheme defaults; empty palettes preserve source tokens exactly. Omitted accent foreground chooses black/white for a changed accent. Literal white and text-on-accent/danger are separate roles. `WithOverrides` copies the resolved palette and applies exact final token replacements without further derivation. The lower-level `NewColorTheme(light, dark)` validates and copies token overrides. The [theme guide](../../app/desktop/boardui/README.md#code-defined-color-themes) defines token lookup, derived accent colors, independent scheme defaults and host injection. The [color catalogue](../../app/desktop/boardui/COLOR-TOKENS.md) lists all current values. Both public controls and native primitive defaults resolve through the selected theme. Source-only resource functions remain independent of overrides. `LoadFonts` registers one embedded font family once per process. `Theme` returns canonical defaults for mature native primitives. `Apply` installs the resolved scheme and in-content popover-menu policy on the view context. There is no runtime web or Node dependency.

Controls return their actual native interactive elements. Callers read native interaction results or bound values. Required state pointers must be nonnil. Repeated/reordered controls and collection items require stable IDs/keys. A scheme change must not reset editor values, selected data, scroll state or caller-owned async work.

The caller owns validation, persistence, requests, cancellation and stale-result rejection. mygo owns mature text editing, native selection, focus/keyboard groups, overlay scoping, dragging and accessibility machinery. There is no substitute editor, generic callback bus or platform renderer adapter in this library.

## Implemented control interfaces and limits

- Button preserves the source's four named variants, three named sizes, leading/trailing glyphs, icon-only accessible names, disabled activation and 0.98 press motion. `DressButton` applies that presentation to an existing native action base. The host's busy state displays the native spinner in the leading slot, exposes busy to accessibility and blocks activation.
- `DressLinkButton` styles an existing native button action. It does not create an anchor, URL navigation or browser behavior.
- Input accepts caller-owned text and preserves label/description/error association, placeholder, password, read-only and disabled behavior in two sizes. Error replaces the visible description. Required paints the source marker and adds an accessible description; the fixed mygo bridge exposes no separate required state. Hidden visual labels retain the accessible name.
- Select accepts a selected-value pointer, stable item IDs and an optional open pointer. It uses a real nonmodal in-window listbox on every OS. PopoverBase, FocusGroup and ButtonBase own outside dismissal, keyboard navigation and activation. The component retains initial selected focus, disabled choices, top-scope Escape, selection and trigger focus restoration. Typeahead, rich option renderers, sections and async option loading remain outside this API. Selection is read from the bound value; no trigger Changed() notification is promised.
- Dialog accepts caller-owned open state and body composition. It derives its surface/motion from the fixed Settings Modal shell, restricts Tab/Shift+Tab, closes the top overlay on Escape, supports outside dismissal and restores its opener. Initial programmatic panel focus follows mygo's recording of that opener. It is not the newer source-unavailable generic Modal API or a port of the complete official multi-page settings application.
- Switch accepts a caller-owned boolean and supports the inspected source md pill/rectangle presentation, disabled state and an optionally hidden visible caption. Checkbox accepts a boolean or the explicit three-state entry point; md/sm, mixed state, disabled state and label ownership are explicit.
- Slider accepts one caller-owned numeric value, bounded range and step. It is a single horizontal-thumb interface with source value-bubble presentation. Range and RTL slider interfaces are not claimed.
- SegmentedControl accepts one caller-owned value and stable items, with solid/plain presentation. Native focus groups own arrows/Home/End/Tab and native buttons own Enter/Space. Arrow focus alone does not commit or clear selection. Disabled items remain noninteractive.

New control implementation is not automatic visual acceptance. Each added state set requires its own actual-React/native comparison and behavior evidence. Application search, code/combobox fields, lists/tables, settings/status/tag views, menus, sheets and previews are explicit `desktopkit`/`desktopui` compositions, not claims that all similarly named official catalogue APIs have been ported.

Informational inline notices pair the active scheme's notification information background and icon tokens with primary text for the title and body.

Unknown variants/resource names and nil mandatory pointers are programmer errors. Fail visibly rather than selecting an approximate fallback skin or missing token. Icon-only actions require a nonempty accessible name. Glyphs are decorative; the action or field supplies its semantic name.

## Native host boundary

Native title bars, traffic lights/GTK caption reservations, Windows caption actions, drag and resize regions, actual maximize state and close lifecycle remain host responsibilities. On macOS, keep real system traffic lights over the application-owned top band. Normal History/Settings SplitViews own this band without a separate application-name row, reserve native button bounds and keep the sidebar toggle outside inert navigation. macOS and Windows setup extend their panel backgrounds behind the caption area. Windows setup places the host caption buttons in the top-right and reserves clearance in each layout. Startup retains a fallback safe-area row. Do not draw replacement buttons for the macOS traffic lights. Linux/macOS system-frame preference and restart semantics remain unchanged. Platform modifiers, tray/system menus, OS file/credential prompts, updater, notifications, shortcuts and clipboard/focus/paste integration remain native.

In-content Select, action and context menus use the shared BoardUI presentation. This does not replace the tray's OS menu or move native integration into the public control package. The separate GUI/Quick Panel/tray/service process roles and exclusive tray clipboard adapter remain intact.

## Rendering and acceptance

The exact source/dependency/font hashes, synthetic data, locale, scheme, viewport in DIP/CSS pixels, device scale and renderer identify a comparison. Reference images must be actual official React output at original device-pixel dimensions. Native software screenshots are view evidence, not OS window/compositor evidence.

The accepted first-slice comparison checks control geometry within 0.5 DIP and empty flat control interiors within 2/255 per RGB channel. Those gates apply only to the named component scenes. Text regions retain full unmasked raster metrics and source typography values; native/Chromium antialias differences remain explicit. New controls, product pages, shadows, animation trajectories and native hosts require their own scoped results. Do not translate/rescale images, hide text mismatches or use page-wide blank-space similarity as parity.

E2E must operate real controls and retain assertions, action timelines, images, geometry and SHA256SUMS. Failure artifacts are immutable. Investigate relevant/repeated failures before delivery; passing retries do not erase them. Native OS keyboard/IME, accessibility bridges, high-contrast policy, GPU material/compositor behavior, multi-monitor focus/paste and Windows memory require explicit platform evidence. Software Unicode editing and builds do not establish those results. User visual approval is separate.

## Coverage and historical evidence

Implementation coverage must distinguish reusable controls, application compositions, exact visual-state acceptance, native platform execution, source-unavailable free entries and Pro. Neither the historical 59-component Kit contract nor the 69 pinned public install items equals the delivered native library count. Do not advertise every component or a full template based on the first settings form.

The previous 0013 three-skin receipts remain historical evidence. They do not define current token choices or acceptance of the unified production presentation. Current architecture and the final component/production receipts are the source of present facts and unexecuted boundaries.

## Asynchronous confirmation completion

Application confirmations block user dismissal while BusyLabel is nonempty.
The caller may clear BusyLabel and Open in the same completion update. A prior
busy frame must not reopen a completed confirmation. The retained complete-view
scenario exercises busy Escape, idle Escape and same-update completion closure.
This UI state contract does not change service cancellation or Engine ownership.

## Application sidebar feedback

Application SidebarNav remains single-select. Pointer hover must not change the
filter or selection. Selected rows use an elevated neutral surface (white with a
hairline border in light mode, raised neutral tint with hairline border in dark
mode) paired with primary ink and an accent leading icon, keeping tag swatches
legible and remaining distinct from the neutral hover wash. A symmetric 12-DIP
leading margin and 12-DIP trailing scrollbar lane keep the 4-DIP translucent
overlay indicator clear of row content and align the 28-DIP quiet chrome toggle
with the navigation grid. Footer actions stay fixed outside the scroll viewport.
Application rows are 30 DIP with 13-DIP `body-2` typography, 16-DIP icons (8-DIP
circular swatches for tags), 8-DIP icon/label gaps, 2-DIP intra-section gaps and
14-DIP section separation. Text, Images and Files appear in the sidebar by default.
Other built-in tags are opt-in from the tag manager. Sidebar tag rows omit redundant `#`
prefixes. Section commands are quiet 24-DIP buttons aligned visually with row
counts; More is a 26-DIP secondary row aligned with navigation labels.
This is an application composition, not a claim that the full official dashboard
sidebar (team/user/announcement/badges) has been ported.
