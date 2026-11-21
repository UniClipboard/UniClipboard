# Native Desktop composition demo

## Current scope and source identity

`unic-desktop-demo` uses the same native BoardUI-backed application composition as the production GUI. `internal/desktopdemo` owns fixture startup/data. `internal/desktopui` owns the renderer and production binding. `internal/desktopkit` contains application-only compositions. The reusable public controls and source-generated resources live in `boardui`.

The visual source is official MIT BoardUI export `ec5c5eb3e29c2441be63b9422cd54f411ef23466`. The [source lock](boardui/assets/source-lock.json) binds reference CSS/TS files, compiler dependencies, Inter and the public helper glyphs. The [application icon lock](internal/desktopkit/icons-source.json) binds 49 functional Remix glyphs. The old three-skin package, platform token snapshots and skin selector are removed.

The page composition has historical lineage in local `main` commit `8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7`; the Quick Panel interaction model has a separate GPUI lineage documented in its [guide](QUICK-PANEL-DEMO.md). Those references explain product structure. They do not select the current visual skin or establish BoardUI pixel parity.

The standalone demo uses 22 fabricated history entries and four devices. Operations remain in memory. Restarting restores fixtures. Copy/send show simulated effects; they do not write the system clipboard or send data. Settings retain session values without configuring the OS or real application. The production entry instead uses the live service binding and its normal setup/runtime lifecycle.

| Current owner | Responsibility |
| --- | --- |
| `boardui` | Public native controls, semantic resources, complete type records, source geometry/motion and licensed assets |
| `internal/desktopkit` | Application lists/tables/navigation, search/tag/status/settings/modal compositions using those resources |
| `internal/desktopui/history.go` | Sidebar, toolbar, grouped list/grid, details, filters and commands |
| `internal/desktopui/devices.go` | Device list/detail, invitations, connection tasks, fingerprints and policy controls |
| `internal/desktopui/settings.go` | Settings navigation and product setting pages |
| `internal/desktopui/tag_manager.go`, `tag_editor.go` | Shared tag registry, assignment, colors, table and application actions |
| `internal/desktopui/search_model.go` | Token/filter semantics, candidate matching, counts and caret-preserving edits |
| `internal/windowchrome` | Native frame policy, reserved caption area and OS window actions |
| `internal/desktopui/quick_panel_*.go` | Quick Panel model, content, image previews and native parent/satellite host |

## Build and open

From the repository root:

```sh
go build -o .build/unic-desktop-demo ./app/desktop/cmd/unic-desktop-demo
.build/unic-desktop-demo --theme light
```

Run one demo process at a time. Other initial states:

```sh
.build/unic-desktop-demo --theme dark
.build/unic-desktop-demo --theme light --page devices
.build/unic-desktop-demo --theme dark --page settings
.build/unic-desktop-demo --theme dark --page quick-panel
```

Pages are `history`, `devices`, `settings` and `quick-panel`. Color modes are `system`, `light` and `dark`. There is one BoardUI presentation on all three OS targets. Old skin environment overrides are not a supported selection path. Setup and unlock preview routes are rejected; production first-install setup remains a separate live flow.

The default main demo client area is 1100×720 DIP. On macOS the native host keeps real traffic lights at (14,18) DIP. History and Settings reach the window top with one 52-DIP toolbar/traffic-light band. The sidebar clears the lights; its toggle stays outside the animated sidebar. When collapsed, the toolbar controls clear the native safe area. There is no separate application-name title row or replacement window button. Windows/Linux native frame behavior remains a host concern. The reusable `boardui-demo` provides a separate, bounded synthetic settings form for component/reference comparison; it does not demonstrate every product page.

## Ordered interaction checks

Run visual/native work only in an explicitly authorized, coordinated environment. Use a disposable fixture process. Do not use a personal product profile, another task's VM or the system clipboard for fixture acceptance.

1. Open History. Inspect sidebar, tags/devices, grouped list, detail and action bar in both schemes.
2. Select text, code, link, image, file and directory entries. Verify the matching preview and metadata. Open/close Quick Look and keep selection stable.
3. Search for `产品`, then `no-match`. Verify matching results and the empty state. Clear filters. Exercise @ devices, # tags, / types, time and extension conditions, candidates, keyboard completion and literal fallback.
4. Combine filters. Verify OR inside a dimension and AND across dimensions. Remove a badge and use Backspace from an empty query. Verify other dimensions remain active and the caret is retained.
5. Change list/grid presentation and card size. Select an item, open its detail and return. Resize narrow enough to use one pane without losing the selected record.
6. Select multiple records. Verify summary, pin, delete confirmation and safe cancellation. Confirm deletion only after the explicit action.
7. Open Devices. Inspect pending/local/linked/suggested devices. Review latency and policy. Pause a link and verify content switches and summary. Toggle the global sync fixture and inspect the notice.
8. Open Invite Device. Inspect the six-digit code and regeneration. Simulate a join. Review its fingerprint before Accept/Reject. Never treat a matching pairing code as automatic trust.
9. Open Connect Device. Verify unknown input stays disabled. Try the error fixture `000-000`, then `482-913`. Exercise approval and rejection. Confirm local-device removal remains blocked.
10. Open the tag editor. Use arrows, Return, Tab/Shift+Tab color selection and Escape. Verify creation, duplicate-name handling, custom-tag detach and protected built-in tags. Open the manager and inspect filtering/sorting, fixed columns and context actions. Preview-only actions must stay distinct from real-service capabilities.
11. Open all settings categories. Change light/dark, language and available controls. Verify disabled/busy state and error/hint layout. The preview retains values only for the session; production persistence is tested separately.
12. Resize to 1000 and 700 DIP widths. Exercise overlay sidebar, one-pane history navigation and narrow device details. Check clipped focus rings, long translated labels, command hit targets and scrolling.
13. Exercise nested Select/menu/dialog controls with pointer, Tab/Shift+Tab, arrows/Home/End, Enter/Space and Escape. One outside Cancel action must reach the underlying control. Closing restores the opener.
14. Open the Quick Panel fixture and perform its [ordered checks](QUICK-PANEL-DEMO.md#ordered-interaction-checks).
15. Verify rejected `--page setup` and `--page unlock` arguments fail before opening mygo.

## Canonical acceptance

Use the existing runner and [evidence contract](../../docs/contracts/desktop-e2e-contract.md):

```sh
make desktop-e2e CASE=boardui-production-migration
make desktop-e2e CASE=boardui-composition
make desktop-e2e CASE=boardui-menus
make desktop-e2e CASE=boardui-surfaces
make desktop-e2e CASE=gui-navigation
make desktop-e2e CASE=gui-approval-reconnect
make desktop-e2e CASE=gui-empty-layout
make desktop-e2e CASE=quick-panel-screenshots
```

These named software-view cases operate actual mygo controls and save synthetic checkpoints. Check the current [catalogue](e2e/CASES.md) for mode and exact coverage. A source declaration or successful build is not a scenario pass. The migration owner records executed reports and unresolved failures in the [production receipt](../../docs/exec/boardui-production-migration.md).

For the isolated public-control comparison:

```sh
python3 app/desktop/boardui/e2e/replay.py --node /absolute/path/to/node
```

The component replay uses actual pinned React components, the same Inter bytes and raw device-pixel images. Its geometry/color gates cover the specified component scenes. They do not automatically accept all product pages, fonts, shadows, animation frames or native hosts. Keep the [component contract](../../docs/contracts/boardui-native-contract.md) and report scope with each result.

## Artifact and platform boundaries

Bind source HEAD/dirty snapshot, input hashes, binary/toolchain identities, font/icon source locks, scheme, locale, viewport, DPI, renderer, action assertions and case status. Preserve original images, geometry, differences, recordings where required and SHA256SUMS. Do not resize a CSS-pixel reference to manufacture a 2x image. Preserve first failures instead of overwriting them with retries.

Scheme changes do not reset product models. Native chrome, keyboard modifier selection, tray/system menus, file and credential prompts, global shortcut registration, clipboard and paste remain native host integration. The shared visual scheme does not make those services identical across platforms.

Software-view Unicode editing does not establish native IME behavior. A macOS-hosted software raster does not establish Windows/Linux native typography, accessibility, focus, compositor output or memory. CJK fallback and long-text matrices require their own evidence. User visual approval must be explicit.

The historical 0013 and related `docs/exec` receipts retain earlier three-skin observations and corrections. Those old results are historical evidence. They are not current launch instructions or acceptance of this unified BoardUI presentation.
