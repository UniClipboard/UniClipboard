# Desktop E2E cases

Generated from `cases/*.txtar`. Run `make desktop-e2e`; select with `CASE=<id>`.

| Case | Mode | Purpose |
|---|---|---|
| [data-binding](#data-binding) | process | History, tags, restore, pairing and reconnect through real services. |
| [gui-approval-reconnect](#gui-approval-reconnect) | view | Legacy GUI fingerprint approval and recovery after service disconnection. |
| [gui-empty-layout](#gui-empty-layout) | view | Legacy GUI empty-history layout and narrow-window bounds. |
| [gui-navigation](#gui-navigation) | view | Legacy GUI navigation, pairing input and destructive-confirmation barriers. |
| [history-search-copy](#history-search-copy) | native | Connect visible user actions to production history and clipboard application. |
| [i18n](#i18n) | view | Exercise five language catalogues through the shared desktop composition. |
| [interaction-motion](#interaction-motion) | view | Button press/release, cancellation, selection reversal and Reduce Motion. |
| [liquid-glass](#liquid-glass) | view | Glass hover feedback, pointer-exit reset and system-menu delegation. |
| [native-menus](#native-menus) | view | System-menu requests, cancellation and selected-value dispatch in the software view. |
| [production-boundary](#production-boundary) | process | Build independent production and demo commands and verify their dependency and embedded catalogue boundaries. |
| [quick-panel-contract](#quick-panel-contract) | process | Target session, clipboard application, cancellation and paste-request ordering with OS fixtures. |
| [quick-panel-screenshots](#quick-panel-screenshots) | view | Quick-panel history, action menu and image-grid compositions in both themes. |
| [quick-panel](#quick-panel) | view | Quick-panel keyboard, search, actions, previews, scrolling and simulated effects. |
| [search-badge-delete](#search-badge-delete) | view | Whole-condition deletion from composite search badges. |
| [search-contract](#search-contract) | process | Literal matching, paging, counts, candidates, repair, cancellation and viewport detail prefetch. |
| [search-disclosure-reduced-motion](#search-disclosure-reduced-motion) | view | Toolbar search disclosure with Reduce Motion enabled. |
| [search-disclosure](#search-disclosure) | view | Animated toolbar search expansion, collapse, filtering and keyboard dismissal. |
| [send-device-menu](#send-device-menu) | view | Send-menu device projection, disabled targets and selected-device dispatch. |
| [sheet-exit-stability](#sheet-exit-stability) | view | Sheet exit frames and input stability after dismissal. |
| [sheet-motion](#sheet-motion) | view | Real Sheet entry and dismissal by Done, Escape and backdrop, live resize/reversal, and inline tag confirmation or cancellation, including Reduce Motion. |
| [sidebar-collapse](#sidebar-collapse) | view | Sidebar closing tail, reopening and interrupted-transition reversal. |
| [source-application](#source-application) | view | Show the captured application's cached icon and name immediately after device and time. |
| [startup-lifecycle](#startup-lifecycle) | process | Ownership, background intent, lightweight launch and process cleanup. |
| [tag-editor](#tag-editor) | view | V1-style inline tag creation, five swatches, removable tinted chips and shared Sheet colors. |
| [tag-fixed-columns](#tag-fixed-columns) | view | Tag-table fixed-column geometry across skins and themes. |
| [tag-manager-platforms](#tag-manager-platforms) | view | Tag management and separate entry assignment across all three skins. |
| [tag-manager](#tag-manager) | view | Tag management, assignment, rename, merge, deletion and protected built-in tags. |
| [token-search](#token-search) | view | Composite search prefixes, counts, completion, literal input, caret handling and Escape. |
| [toolbar-alignment](#toolbar-alignment) | view | Toolbar menu-trigger alignment against adjacent controls. |
| [unified-scrollbars](#unified-scrollbars) | view | Scrollbar visibility during hover, scroll, idle and nested preview interaction. |

## data-binding

Desktop data binding

Steps: Execute the registered production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance desktopui data-binding
```

## gui-approval-reconnect

Gui approval reconnect

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Explicit approval information stays visible and the view recovers after reconnection.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance gui gui-approval-reconnect
```

## gui-empty-layout

Gui empty layout

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Empty state fills the body and focused controls remain visible across skins and themes.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance gui gui-empty-layout
```

## gui-navigation

Gui navigation

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: All three skins and both themes retain navigation, normalized pairing input and safe dismissal.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance gui gui-navigation
```

## history-search-copy

Search and copy real history through the native desktop

Steps: Start an isolated desktop, submit two text entries, search for one, copy it, then clear the search.

Expected: Only the matching entry remains; copying writes exact directory-clipboard bytes; clearing restores both entries.

Forbidden: Fixture rows, system clipboard writes, missing screenshots or missing continuous recording.

```text
start
submit selected 'Desktop evidence selected needle'
submit other 'Desktop evidence other entry'
ui-wait listItem 'Desktop evidence selected needle'
ui-wait listItem 'Desktop evidence other entry'
screenshot history
ui-key CmdOrCtrl+F
ui-wait comboBox '搜索历史'
ui-set comboBox '搜索历史' needle
ui-wait listItem '~.*selected needle$'
ui-absent listItem 'Desktop evidence other entry'
screenshot filtered
ui-click listItem '~.*selected needle$'
ui-key Enter
board-equals 'Desktop evidence selected needle'
screenshot copied
ui-set comboBox '搜索历史' ''
ui-wait listItem 'Desktop evidence other entry'
screenshot restored
```

## i18n

Desktop languages

Steps: Select each language through settings, navigate to history, filter files, verify switches have one visible row caption and retain accessible names and mouse/keyboard behavior, and render quick-panel actions across three skins and both themes.

Expected: Navigation and actions use the selected language; content and filter identities remain unchanged; catalogues and format arguments are complete.

Forbidden: Missing translations or translated user content.

```text
acceptance desktopui i18n
```

## interaction-motion

Interaction motion

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Geometry and rendered selection feedback follow the complete interaction sequence.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui interaction-motion
```

## liquid-glass

Liquid glass

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Light and dark view rasters show reactive rims and reset without stale hover feedback.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui liquid-glass
```

## native-menus

Native menus

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Menu requests and returned commands match each exercised filter and action.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui native-menus
```

## production-boundary

Production and demo module boundary

Steps: Inspect production, shared renderer and translation dependencies; build both commands; run demo help without opening a GUI.

Expected: Production imports desktopui and i18n, excludes desktopui and its fixtures, and embeds locales from i18n; the demo command builds and prints help.

Forbidden: Demo dependencies in production, translations in desktopui, skipped checks, or native window launch.

```text
acceptance startup production-boundary
```

## quick-panel-contract

Quick-panel system ordering

Steps: Execute the registered production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance quickpanel quick-panel-contract
```

## quick-panel-screenshots

Quick panel screenshots

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Six composite PNGs retain source identity, image hashes and view rectangles.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui quick-panel-screenshots
```

## quick-panel

Quick panel

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: All original panel scenarios retain their selected entries and expected simulated effects.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui quick-panel
```

## search-badge-delete

Search badge delete

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Backspace and remove actions reset only the selected condition dimension.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui search-badge-delete
```

## search-contract

History search contract

Steps: Execute the registered production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance desktopui search-contract
```

## search-disclosure-reduced-motion

Search disclosure reduced motion

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Disclosure reaches its expected geometry without spatial transition.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui search-disclosure-reduced-motion
```

## search-disclosure

Search disclosure

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Search width changes without moving the command capsule or losing input behavior.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui search-disclosure
```

## send-device-menu

Send device menu

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Only valid device choices produce the expected send command.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui send-device-menu
```

## sheet-exit-stability

Sheet exit stability

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Light and dark exit rasters preserve stable content and release dismissed input.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui sheet-exit-stability
```

## sheet-motion

Sheet motion

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Real Sheets preserve entry geometry, centered resizing and completed exits; inline tags use no Sheet, Enter attaches, and Escape/click-away cancel without mutation across all theme/motion combinations.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui sheet-motion
```

## sidebar-collapse

Sidebar collapse

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Animated content geometry remains continuous and preserves sidebar state.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui sidebar-collapse
```

## source-application

Source application in history detail

Steps: Project service history metadata, render the complete history view across skins and themes, and switch records.

Expected: Cached icons render; absent or malformed icons use a generic icon; absent names use the identifier; long names remain bounded.

Forbidden: Stale source metadata after selection changes, replacement by local application identity, hidden device/time, or native-window claims from software-view evidence.

```text
acceptance desktopui source-application
```

## startup-lifecycle

Desktop startup and service lifetime

Steps: Execute the registered production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance startup startup-lifecycle
```

## tag-editor

V1 inline tag editor and colors

Steps: Open production history composition on three skins and two themes; create, recolor, attach, remove, cancel and use keyboard navigation.

Expected: Inline ComboField and palette preserve focus, names, associations and shared color state; rendered checkpoints are retained.

Forbidden: Checkbox assignment Sheet, duplicate creation, mutation on color choice or cancel, skipped subcases or native claims.

```text
acceptance desktopui tag-editor
```

## tag-fixed-columns

Tag fixed columns

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Complete management sheets preserve column alignment and row geometry.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui tag-fixed-columns
```

## tag-manager-platforms

Tag manager platforms

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Built-in operations remain protected and assignment updates the shared registry in both themes.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui tag-manager-platforms
```

## tag-manager

Tag manager

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: All original management operations preserve their history and shared tag-registry invariants.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui tag-manager
```

## token-search

Token search

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Fourteen scenarios across three skins and both themes preserve the complete search contract.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui token-search
```

## toolbar-alignment

Toolbar alignment

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Both themes align menu triggers with the pin button at the expected geometry.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui toolbar-alignment
```

## unified-scrollbars

Unified scrollbars

Steps: Execute the registered scenario and all its theme, skin and interaction subcases.

Expected: Three skins and both themes preserve edge indicators and independent nested scroll ownership.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui unified-scrollbars
```
