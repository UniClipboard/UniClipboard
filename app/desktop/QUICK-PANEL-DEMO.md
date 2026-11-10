# GPUI quick panel fixture acceptance

## Reference identity

The port uses the repository's cached `origin/main` at `885546c7cf6f6f0d2076104efd1d90d9f73d9744` (2026-11-08).
Local `main` is older: `20c5ddcaf52e3504964b348908d34d3ccf09307e` (2026-10-26).
A live remote-head check failed because the Lody GitHub credential helper could not acquire credentials.
The cached reference is immutable; this work does not assert that it is the current remote head.
The existing desktop fixture keeps its separate reference.

| GPUI source at the reference | mygo fixture |
| --- | --- |
| `crates/quick-panel-core/src/geometry/window_pair.rs`, `apps/quick-panel/src/app/window.rs` | `quick_panel_window.go`: history and parent-owned satellite windows |
| `apps/quick-panel/src/ui/history/{mod,row,search_row,footer,suggestions,empty_page}.rs` | `quick_panel_view.go`: compact history, search, suggestions, result rows and empty states |
| `crates/quick-panel-core/src/state/{keys,search,suggest,menu,preview,session}.rs` | `quick_panel_model.go`: fixture state, delayed search/preview, focus zones, actions and simulated effects |
| `crates/quick-panel-core/src/{actions,selection,grid,empty_page}.rs` | `quick_panel_model.go`, `quick_panel_view.go`: enabled action traversal, bounded selection, image paging and relaxation proposals |
| `apps/quick-panel/src/ui/{view,image_preview}.rs` | `quick_panel_view.go`, `quick_panel_image.go`: typed preview and image fit/actual-size/pan |
| `apps/quick-panel/assets/themes.json` | `quick_panel_view.go`: default light/dark GPUI colors and fixed geometry |

These reference paths belong to the other branch. Inspect them with `git show 885546c7:<path>`.
See the [architecture](../../docs/architecture/overview.md#gpui-quick-panel-fixture) for the implementation boundary.

## Build and run manually

```sh
go build -o .build/unic-desktop ./app/desktop/cmd/unic-desktop
UNIC_UI_SKIN=macos .build/unic-desktop --demo --page quick-panel --theme dark
```

Use `--theme light` for the light preset.
Use `--quick-state locked|disconnected|empty` for the other initial states.
The locked fixture accepts only `demo`. Reconnect replaces the disconnected state with local fixtures.
The demo contains 24 fabricated entries, including 15 images, rich text, code, links, multiple files, a favorite and a lost payload.
The footer's `TextEdit` target is fabricated.
Paste, copy-entry, send, open, reveal and host commands append simulated effects. They do not invoke the OS or Engine.
The normal text field retains ordinary text editing and copying selected search text.
Closing the panel hides both windows. On macOS, reactivate the demo from the Dock to reopen it. Quit exits the demo.

## Ordered interaction checks

1. Confirm a 380 × 366 frameless history panel. Confirm a 48-point search header, 30-point result rows, and 34-point footer.
2. Wait 500 ms. Confirm that the preview appears beside the first result. Press Down. Confirm that the preview follows after 120 ms.
3. Move the pointer over another row. Confirm that the preview follows the pointer while the blue keyboard selection stays. Press Down. Confirm that the keyboard controls the preview again.
4. Scroll below the first nine results. Confirm that later list rows have no shortcut placeholder. The list's Command/Ctrl+1–9 bindings continue to identify the first nine search results.
5. Type `/图片`. Confirm suggestions without automatic acceptance. Press Tab. Confirm an image filter and a 3 × 3 grid. Use arrows and the wheel. Confirm that numbered shortcuts identify the current grid cells.
6. Type `#工作`, `@工作电脑`, `wb`, or `今天`. Accept suggestions with Tab. Use Up from the first result to enter suggestions. Use Enter to accept the focused suggestion. Use Down to return to results.
7. Accept `/文本`, then `/文件`. Confirm OR within the type dimension. Add a tag or device. Confirm AND across dimensions. Remove a chip. Press Backspace in an empty input. Confirm removal of the last condition.
8. Search for absent content. Confirm the no-match page. Combine `/图片` with `#工作`. Confirm relaxation proposals and apply one with Enter.
9. Press Command/Ctrl+K or right-click a result. Confirm that actions replace the satellite preview. Confirm that Up/Down wrap over enabled actions. Open Send to Device. Confirm that the local device is absent and the offline device is disabled. Press Escape to return one level.
10. Exercise Enter, Shift+Enter, Command/Ctrl+Enter, Command/Ctrl+C and Command/Ctrl+1–9. Confirm simulated hide, plain-text, and keep-open behavior. Select search text before copying. Confirm that this copies search text.
11. Favorite and delete entries. Confirm immediate in-memory updates. Select the lost file. Confirm that restore shows an error and does not produce a paste effect.
12. Hover an image preview. Toggle actual size and fit. Double-click to toggle zoom. Drag while at actual size. Confirm bounded pan and reset when the image changes.
13. Press Escape with suggestions, with filters, and with an empty search. Confirm suggestion dismissal, filter reset, then panel hide. In the locked state, Escape clears the passphrase before hiding.
14. Move the parent near each display edge. Confirm satellite flipping and clamping. Click satellite actions. Confirm that the history input retains keyboard focus. Activate another application. Confirm that both windows hide.
15. Repeat with light and dark themes. Record native evidence against the manifest and source hashes.

On Windows and Linux, use Ctrl where macOS uses Command.
The sample has no global activation shortcut. Launch it explicitly for each session.

## Repeatable non-native acceptance

```sh
app/desktop/scripts/quick-panel-e2e.sh
```

The script does not launch an application or browser.
It writes `.build/quick-panel-e2e/quick-panel-e2e.json`, `test.log`, `manifest.json`, and a build of `unic-desktop`.
The JSON receipt contains scenario results, accessibility geometry and simulated effects.
The manifest records the reference, base HEAD, individual source hashes, executable hash, toolchain and command.
Use the source hashes to identify uncommitted changes; HEAD alone does not identify this implementation.
Set `UNIC_UI_TEST_ARTIFACTS` to choose another output directory.

These view-level scenarios exercise the real mygo layout, focus, input, timers, scrolling and rendering with a test clipboard.
They are not native window-manager, OS focus, screenshot-comparison or automatic-paste acceptance.

## Parity boundaries

The fake backend implements local substring matching, fixed relative-time fixtures and fixture-catalog suggestions.
It does not port the daemon's search index, arbitrary natural-language date grammar, arbitrary pinyin transliteration, live reconnection or service timing.
The rich-text fixture uses mygo's existing HTML parser and read-only rich-text editor. It does not test GPUI's full HTML compatibility.
The existing Kit supplies icon glyphs. GPUI font rasterization, exact icon paths and native window shadows can differ.
The satellite uses mygo's maintained nonfocusable Flyout for parent ownership, edge flipping and placement.
It does not use GPUI's native window implementation. Multi-monitor placement and native focus require the manual checks above.
Global hotkeys, modifier double taps, real target capture, clipboard restore, OS paste, automatic reopening and persisted settings are outside this fixture.
No native visual or interaction acceptance was performed during implementation.
Do not claim complete one-to-one visual parity from the view-level test result.

## Explicit offscreen screenshots

When the user requests images without opening the desktop, export the real mygo view rasters:

```sh
UNIC_QUICK_PANEL_SCREENSHOTS=1 UNIC_UI_TEST_ARTIFACTS="$PWD/.build/quick-panel-screenshots" \
  go test ./app/desktop/internal/desktopdemo -run '^TestQuickPanelScreenshotsE2E$' -count=1 -v
```

The six PNGs cover history with text preview, actions, and the image grid with image preview in both themes.
Each PNG places the independently rendered history and satellite views on a neutral canvas at 2x scale.
`screenshots.json` records base HEAD, dirty-source file hashes, PNG hashes, view rectangles, scale and export method.
This export uses the existing `ui.Tester.Image` and PNG encoding path. It never calls `App.Run` or creates native windows.
It demonstrates the current view rendering. It does not establish native window shadows, OS focus or display-edge placement.
