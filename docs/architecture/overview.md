---
paths:
  - go.mod
  - engine
  - internal
  - plugin
  - plugins
  - app/desktop/boardui
  - app/desktop/cmd
  - app/desktop/e2e
  - app/desktop/internal
  - app/service
  - bindings
  - app/expo
  - app/android
  - app/ios
  - app/mac/Package.swift
  - app/mac/Sources
  - .github/workflows
  - e2e/cli
  - tools/bench
  - tools/docscheck
verified: 77a5c9124adac5d57571a484c91c7231ecc4203f
---
# 仓库布局与依赖边界

unic 使用 monorepo：平台无关的 Go 核心位于根目录，各端宿主位于 `app/<平台>/`，由宿主装配核心并负责系统集成。

## 现状

- 根 `go.mod` 的模块路径为 `github.com/uniclipboard/unic`。移动端的 Go `gomobile` 绑定作为嵌套子模块位于 [`bindings/`](../../bindings/go.mod)（`bindings/mobile`，使用本地 `replace github.com/uniclipboard/unic => ..` 而不检查入库 `go.work`）；根模块 `go test -race ./...` 仅覆盖核心、插件和桌面端的 Go 测试。
- `engine/` 是公开 Go SDK，管理生命周期、设备信任、历史、处理流水线与同步调度；根 `internal/` 放核心实现及内置插件。`plugin/` 提供独立进程插件 SDK、RPC 和设置 Schema 工具，`plugins/` 放插件示例及开发说明。
- Root `internal/v1migration/` provides old-history migration through the in-process `unic migrate` command. It includes desktop profile discovery and platform credential reads, such as macOS Security.framework. Engine does not import it. There is no separate migration program. CLI and Desktop setup share `Source.Run`, including automatic safe tag and color completion on every migration run.
- `app/service/` contains the UI-free CLI and service. `app/desktop/` contains `unic-desktop`, reusable native BoardUI controls, application composition and native integration. Shared local protocol, configuration, paths and content conversion live in root `internal/local*` packages.
- `engine.Settings.Validate` validates Engine business settings. `internal/localconfig.Config.Validate` validates host settings and calls it. Engine does not parse TOML or watch configuration files.
- `app/mac/` 是独立的 Swift Package，保留 SwiftUI 界面与插件设置表单的实验代码，尚未连接 daemon；该客户端路线已取消，停止推进服务接入，现有实验画面不作为最终界面规格。`app/expo/` owns the shared Expo 57 application for Android and iOS, embedding the Go Engine in-process via [`bindings/mobile/`](../../bindings/mobile/engine.go) and the Expo Local Module `app/expo/modules/unic-engine` (with an explicit `"unavailable"` shell fallback when the native module is absent in JS-only runs). `app/android/` and `app/ios/` retain platform guidance only. Every application directory has its own README and AGENTS.md.
- Before starting the Engine, the Android host registers a Java network interface provider through the mobile binding. Tailcat reads a fresh interface snapshot from this provider because the Android app sandbox restricts Go's direct interface enumeration.
- CI runs path-scoped workflows so each PR only executes affected checks (see [PR check trigger boundaries](releases.md#pr-check-trigger-boundaries)). `pr-checks.yml` classifies changed files via `tools/ci/pr_changes.py` to gate lint/formatting, race tests + builds, production headless E2E, and Desktop E2E smoke shards independently; `desktop-e2e-nightly.yml` runs the full six-shard Desktop E2E catalogue, timing report, and cross-platform native acceptance on a nightly schedule and on demand; `repo-docs-check.yml` validates repository docs (`go run ./tools/docscheck`), `docs-check.yml` validates `docs-site/`, `release-checks.yml` validates release metadata/notes/tooling, and `desktop-benchmark.yml` provides scheduled nightly and manual (`workflow_dispatch`) Win/Mac resource benchmarking across four macOS and Windows runners (see [Win/Mac 桌面资源 Benchmark](desktop-benchmark.md)).
- `check-expo.yml` verifies mobile bindings, TypeScript, Expo prebuild, and JavaScript bundles on matching PRs. Native iOS and Android builds and simulator/emulator acceptance run on matching pushes to `main`, on schedule, and on manual dispatch.
- Desktop PR CI runs the curated smoke suite (`SUITE=smoke`) plus any isolated `cases/*.txtar` modified in the PR across two duration-balanced shards (twenty-minute limit per job). Full six-shard isolated Desktop E2E (thirty-minute limit per job) and six-target Win/macOS/Linux native acceptance run nightly in `desktop-e2e-nightly.yml`. Ubuntu package bootstrap has a separate five-minute step limit with retry, uses the official HTTPS archive for direct sources and the runner's file-backed mirror list, and bounds APT connection/data waits and retries. See the [Desktop E2E contract](../contracts/desktop-e2e-contract.md).

## 不变量

- 应用依赖核心，根 `engine/`、`internal/`、`plugin/` 不得依赖任何 `app/*`。
- Application-specific Desktop packages live in `app/desktop/internal/`. The reusable native control package is `app/desktop/boardui`; it has no Engine or service ownership. Core packages and UI-free commands must not depend on either UI path. Cross-platform business capabilities use the public SDK or protocol. The existing migration exception is root `internal/v1migration/`.
- `engine` 的传递依赖不得包含应用包、mygo、fsnotify、TOML 解析器、tailcat 会话层 `internal/session` 或传输层 `internal/tcnet`。`engine/boundary_test.go` 检查这条边界；内置后端由宿主显式装配。
- 系统剪贴板、托盘、GUI、本机 HTTP/socket 服务和配置文件管理属于宿主；核心只接收类型化设置和宿主通过 SDK 提交的观察、应用结果。
- 根核心包不依赖 `bindings/` 或 `app/*`。`bindings/` 作为嵌套 Go 模块封装 `engine/`、`builtin.tailcat`、`builtin.classify`、`builtin.text` 与 `internal/v1migration/` 供移动端调用；构建脚本用 `GOWORK=off`（不提交常驻 `go.work`）。
- 每个应用维护自己的 README 与 AGENTS.md，根文档维护总览和跨端约束。

## 代码地图

| 路径 | 职责 |
|---|---|
| `engine/engine.go`、`types.go`、`options.go` | SDK 入口、设置与校验 |
| `engine/boundary_test.go` | 公开 Engine 的传递依赖边界测试 |
| `internal/store/`、`identity/`、`wire/`、`clip/` | SQLite、设备身份、线协议与内容模型 |
| `internal/session/`、`tcnet/`、`rendezvous/`、`plugins/tailcat/` | 内置 tailcat 后端的会话、传输、配对码与装配 |
| `internal/plugins/builtintext/`、`luavm/`、`luaplugin/`、`pluginproc/`、`peerbackend/` | 内置文本处理、Lua 运行时与插件适配 |
| `internal/localapi/`, `localconfig/`, `localpaths/`, `localcontent/` | Shared local protocol, configuration, paths and content conversion |
| `internal/clipboardintegration/`, `servicelifecycle/` | UI-independent clipboard state machine and service generation control |
| `internal/observability/` | Service-owned logs, metrics, analytics and deployment binding |
| `internal/v1migration/` | Old-history migration for `unic migrate`, including platform credential reads |
| `internal/servicelaunch/`, `webtransfer/`, `syncdiag/` | Shared unicd launch primitives; temporary-transfer web protocol; sync diagnostics records |
| `plugin/`、`plugins/` | 插件 SDK、RPC、设置 Schema 与示例 |
| `app/service/cmd/`, `app/service/internal/` | UI-free CLI, service entry point and host assembly |
| `app/desktop/cmd/`、`app/desktop/internal/` | Go 桌面命令入口与宿主实现 |
| `app/mac/` | 停止推进的 SwiftUI 实验原型与设置表单样例 |
| `bindings/` | Nested Go module exposing `gomobile`-compatible Engine & setup/v1 migration bindings (`UnicCore.xcframework` / `unic-engine.aar`) |
| `app/expo/` | Shared Expo 57 mobile application (`modules/unic-engine` native host, `src/engine/` client, Setup/migration UI, and JS-only shell fallback) |
| `app/android/`、`app/ios/` | Platform integration guidance; no separate application projects |
| `e2e/cli/`、`tools/docscheck/` | CLI 多进程端到端验证与文档检查 |
| `e2e/real/` | Node-configured real-machine CLI testing over local processes and SSH; see [architecture](real-machine-testing.md) |
| `tools/bench/` | Win/Mac 桌面资源 Benchmark 工具链（`unic-bench` 与 Parallels VM 编排）；see [architecture](desktop-benchmark.md) |

## 规格与背景

- SDK 边界：[Engine 公共契约](../contracts/engine-contract.md)、[SDK 扩展类型契约](../contracts/sdk-extension-contract.md)。
- 宿主装配：[后端迁移契约](../contracts/backend-migration-contract.md)。
- 业务地图：[流水线与本机历史](pipeline.md)、[设备身份、信任与配对](identity-and-trust.md)、[Win/Mac 桌面资源 Benchmark](desktop-benchmark.md)。
- 应用说明：[桌面](../../app/desktop/README.md)、[macOS 预览](../../app/mac/README.md)、[Expo](../../app/expo/README.md)、[Android](../../app/android/README.md)、[iOS](../../app/ios/README.md)。

## Desktop layout demo

`unic-desktop-demo` opens the native desktop layouts with in-memory fixtures.
The composition follows local `main` commit `8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7`.
`app/desktop/internal/desktopdemo` owns preview startup and fixture builders. `app/desktop/internal/desktopui` owns shared page composition and production service binding. The independent `unic-desktop-demo` command supplies explicit preview records from `desktopui/preview`; `unic-desktop` imports neither the demo launcher nor fixture builders.
The public native `boardui` package supplies shared controls and source-generated semantic resources. `internal/desktopkit` supplies application-only compositions over those controls and mature mygo behavior primitives. Main GUI, setup, Quick Panel, preview and component demo use one BoardUI content presentation on Windows, macOS and Linux. The former three-skin package, token snapshots and skin selector are removed. Color mode still supports system/light/dark. See [native BoardUI presentation](#native-boardui-presentation).

The main demo keeps its 1100×720 default window and shared history/device/settings model. Desktop history uses the outlined Remix star for the Favorites sidebar, commands and item indicators. The demo keeps synthetic favorites in memory. The production binding persists them in Engine history. Native window decoration and reserved caption areas belong to `internal/windowchrome` and the OS host. On macOS, History and Settings own one unified toolbar/traffic-light band at the window top. There is no separate application-name title row. Real system traffic lights remain; the sidebar and collapsed toolbar clear their native safe area. Setup/startup retain a fallback drag region; native title-bar presentation is hidden by default. Content controls no longer select platform glass surfaces, stroke widths, typography or system-menu styling. In-content Select/action/context menus use the BoardUI presentation; tray/system menus remain native.

Sidebar navigation keeps one caller-owned selection. Selected rows use an elevated neutral surface with primary labels and an accent leading icon (leaving tag swatches and device dots legible), distinct from the subtle neutral hover wash. A quiet 28-DIP chrome button toggles the sidebar without competing with navigation selection. The scroll body pairs a 12-DIP leading margin with a symmetric 12-DIP trailing lane for the 4-DIP translucent overlay scrollbar so row content and counts stay clear. Footer actions stay outside that scroll body under a hairline divider. Navigation uses 30-DIP rows, 13-DIP `body-2` typography, 16-DIP icons (8-DIP circular tag swatches), 2-DIP intra-section gaps, 14-DIP section separation, quiet 24-DIP section commands aligned with row counts, and a label-aligned secondary More row. Built-in content tags default off in the sidebar (opt-in from the tag manager) and sidebar tag labels omit redundant `#` prefixes.

The history toolbar retains command hit targets, search disclosure and list/grid switching. The search model in `desktopui/search_model.go` handles @ devices, # tags, / types, on: time and ext: extensions; candidates, counts, badges, keyboard completion, literal fallback and caret-preserving edits share the existing filter state. Escape closes candidates, clears search, then collapses empty disclosure. Badge removal resets its dimension; Backspace in an empty query removes the last badge while preserving other dimensions. Native modifiers remain host-specific.

The history tag registry is shared by sidebar visibility, search candidates, list/detail badges and assignment. The management table preserves filtering, sorting and fixed-column behavior. Column growth remains bounded by the viewport and keeps a flexible trailing column where needed. Application modals/sheets use mygo focus scopes and source-bound surfaces; they are product compositions, not claims of the source-unavailable generic BoardUI Modal API.

Below 900 DIP (`desktopkit.SidebarOverlayWidth`) the sidebar collapses into an overlay drawer with layered depth enter/exit motion and gentle canvas recede matching the floating detail drawer (respecting `ReduceMotion`, with transparent light-dismiss and Escape handling). Below 760 DIP history list/detail share one pane. Device management opens as a full-window page (`devices`, sibling to Settings) combining a 2-column device card overview with a right-hand inspector (collapsing to single-pane below 760 DIP) and a focused task Sheet for pairing/adding devices; window-scoped Escape dismisses active menus/dialogs/sheets or inline rename first, steps back from single-pane device detail to the overview, and returns from the overview to history even when no control holds focus. Approving a pending device still requires explicit fingerprint review. The animated sidebar viewport owns clipping while its inner body owns padding, so collapse does not leave a final padding jump.

Application scroll, list and grid factories share centralized indicator policy in `desktopkit/scroll_indicator.go`: show after position changes and hide after 600ms idle; hover alone does not reveal indicators. Each container owns its own state. History list/grid retain a scrollbar gutter, stable IDs and viewport reporting. The 52-DIP history toolbar integrates a centered unified search-and-filter omnibar (`SplitViewProps.SearchCentered`, `TokenSearchFieldProps.Unified`) combining live token search (`#`, `@`, `/`, `on:`, `ext:`), inline removable condition chips and a built-in filter funnel popover with live prospective counts, replacing the former standalone second filter row. Toolbar density adapts deterministically to effective available content width (`desktopkit.ToolbarLayout`, accounting for docked sidebar, zoom scale, macOS traffic-light clearance and Windows host caption buttons): compact widths (`< 600` DIP) tighten spacing, hide decorative shortcut pills and compact the single-pane back button; narrow widths (`< 480` DIP) collapse idle search to a compact trigger (or accent active-condition badge) leaving draggable titlebar space, and expand into a full-width search takeover with a collapse button (`收起搜索`) on click, `⌘F`/`Ctrl+F` or filter interaction while preserving host caption controls and collapsing cleanly on empty Escape or outside blur without discarding active filters; ultra-narrow zoomed widths (`< 360` DIP) fold secondary history commands into the overflow menu. Switching between list and grid plays a directional horizontal slide (`±28` DIP, respecting `ReduceMotion`) while smoothly revealing/hiding the compact grid card-size slider. In history list mode, `GroupedList` renders inset rounded rows (`ItemListItem.CardRow`) with soft blue tinted border selection, source application icons with content-kind corner badges (or image thumbnails), deduplicated single-line excerpts, user tags and hover quick copy, paired with a detail pane that shares the semantic link card, 3-column facts strip, subtle micro-slide switch and floating pill dock. In history grid mode, `GroupedGrid` renders semantic source-first cards (origin application icon/name or content-kind fallback, deduplicated short text, inset code/link/file previews, full-bleed image thumbnails, user tags, pinned mark and hover quick actions), and clicking a card opens a floating inset full-height detail card (`8` DIP margin, `12` DIP radius) covering the trailing titlebar band (`SplitViewProps.Drawer`) with layered depth enter/exit motion (exit-retained slide + gentle canvas recede, respecting `ReduceMotion`), a 48-DIP integrated top bar, pager, direction-aware micro-slide content switching, semantic preview, compact facts, tags, delivery status and the floating pill action dock.

The demo contains history list/grid/detail, device management and settings. Its operations remain in memory; it creates no API client, clipboard writer, shortcut registration or persisted settings. The production `unic-desktop` uses the same renderer with its live service binding. Build success alone establishes no native visual or interaction acceptance. See the [demo guide](../../app/desktop/UI-DEMO.md), [component contract](../contracts/boardui-native-contract.md) and [production migration receipt](../exec/boardui-production-migration.md).

## Live desktop composition

A fresh Desktop profile opens the first-install setup in the independent GUI process before starting the tray-owned service or clipboard adapter. This gate applies to both normal and lightweight startup. The welcome slogan is “让每一次复制更有用”. It offers optional, offline v1 history migration and shows importing, verification, and complete/partial/cancelled/failed results. Setup and history reuse one native window. Setup uses a 780 × 540 content area (with a compact responsive layout below 700 DIP); after the user chooses entry or skips migration, the same window switches to history and expands centered around its current midpoint (clamped to the display work area, unless maximized or full screen) to 1100 × 720. After setup completion, the normal desktop opens the shared history composition directly. It has no recurring unlock gate.
`app/desktop/internal/setupflow` owns the profile-scoped `desktop-setup.json` marker and calls the existing v1 importer. `app/desktop/internal/setupui` renders the [setup flow](../../prototype/setup.html) with the same BoardUI-backed application composition. `session_gui.go` serializes setup, runtime startup, and later migration ownership.
A pre-existing regular, nonempty `unic.db` without the new marker identifies an established v2 profile, including one with empty history. A pending marker takes precedence over database presence, so interrupted imports return to setup. Source selection is saved; imported-record mappings remain owned by the importer. Invalid setup state is an error rather than an implicit completion.
Settings > Storage > Migrate from v1 asks the tray to serialize runtime pause, close the quick panel, release shortcut/adapter ownership and stop its owned service before showing migration. Pending setup persists before import. It does not automatically stop an independently running service during migration. The importer requires exclusive Engine ownership and never applies imported content to the system clipboard.
See the [setup execution receipt](../exec/0026-first-install-setup.md) for acceptance limits.
Quit uses a window-scoped platform command-Q shortcut, including when no view control has focus.
`desktopui.NewDesktop` initializes empty service-backed state, without fixture data.
`live.go` serializes local socket work and applies completions on the view thread.
`live_wake.go` binds weak, thread-safe mygo window services to each production view. Every successful bounded result publication and relevant service event invalidates its owning window. The main history and quick panel no longer request a frame every 200 ms. Their models still apply queued completions only on the view thread. Independent deadlines wake the window for the 30-second recovery check, failed-page retry and visible invitation expiry. Search debounce, blur and preview timers retain their existing view deadlines. Closing a binding cancels its background work and stops its wake timer.
It owns query pagination, reconciliation, bounded content previews and revision-checked history mutations.
The event stream requests fresh snapshots; an independent 30-second revision check also covers lost events without a frame heartbeat.
Initial reads and changed queries show loading. Timer/event reconciliation and pagination keep existing history visible without adding a loading row. The quick panel uses the same foreground-loading state. Cancellation and completion clear only the matching read generation.
The history list has no manual refresh button; event updates and periodic reconciliation refresh it automatically.
Selected single-image files use the same image frame and Quick Look as image records, with their filename and actual dimensions. Their stored file kind, type filters and file-copy semantics stay unchanged. Preview reads are limited to 32 MiB and 16 million decoded pixels; unsupported, corrupt, oversized and multi-file payloads retain the file list.
Refresh preserves loaded page depth. Query changes discard results for obsolete filters.
`live_devices.go` binds existing pairing, approval, candidate and removal operations.
`live_settings.go` binds the existing General, Sync and Storage settings; the persistent tray owns native login registration.

### Desktop languages

The shared desktop composition supports Simplified Chinese (`zh-CN`), English (`en`), Japanese (`ja`), Brazilian Portuguese (`pt-BR`) and Russian (`ru`). General settings provides a language selector with native language names. The default is Simplified Chinese; an empty stored language also selects it. Unsupported nonempty language codes fail configuration validation.

`desktop.language` persists through the existing revision-checked configuration API. A successful save updates the main window and quick panel through the normal configuration refresh, without a process restart. The tray watches the same file to rebuild its labels. Device notifications use the current configured language. The demo keeps its selection in memory.

`app/desktop/internal/i18n/i18n.go` resolves application-owned source strings from embedded `locales.json`. Navigation, settings, device sheets, history controls, quick-panel controls and actions share the catalogues. Action result dialogs and progress, cancellation, failure and completion notifications use the same catalogues, including clipboard-copy outcomes and link controls. Source strings are stable catalogue IDs; internal filter values and query syntax remain unchanged. Content, filenames, device names and user tags bypass translation. Service-generated history titles carry an explicit marker so they can be translated without translating user text. Missing catalogue text falls back to the Chinese source. No translation request leaves the device. Standard numeric/ISO timestamps remain unchanged; Today and Yesterday headings are translated.

The translation package depends only on the standard library. Production UI, tray/notifications and isolated previews share it. The catalogue is embedded from `app/desktop/internal/i18n/locales.json` into each executable that imports it. No demo package owns production translations.

Run `make desktop-e2e CASE=i18n` for five-language, light/dark software-view acceptance of the shared BoardUI composition. OS host selection is separate from presentation and does not create alternative skins. The `data-binding` case verifies persistence with a real isolated service and a newly created desktop binding. See the [i18n receipt](../exec/desktop-i18n.md). Native tray, notification delivery, OS focus and platform visual acceptance require separate native evidence.
The service lifecycle, exclusive clipboard adapter, tray and normal/lightweight modes remain defined by the [headless architecture](headless.md).
Global literal text/file-name search, rich-text classification, extension/source/tag/time filters, paged candidates and exact prospective counts use the Engine-owned [search interface](../contracts/history-search-contract.md).
The main editor keeps word/all-tag semantics; the quick panel keeps phrase/any-tag semantics. Both reject obsolete read generations and use separately cancellable search/count/content reads.
The GUI, tray menu and global shortcut request the same independent real-data quick-panel process. Copy/delete call the existing service. OS paste follows the [quick-panel system contract](../contracts/desktop-quick-panel-contract.md) and requires separate native acceptance. Semantic classifiers, favorites, global tag rename/merge/deletion and standalone empty-tag creation, richer device policies/status and direct history sending remain gaps.
The normal UI never falls back to fixture data or simulated effects.
See the [execution receipt](../exec/desktop-live-data.md) for nonvisual process acceptance and native limits.

### History detail prefetch

The production main history list, history grid and search results share a viewport-owned detail cache in `desktopui/live_details.go`. The application list/grid composition supplies inclusive item indices through `ListState.VisibleRange`, excluding group headers and accounting for grid columns. Every visible item and ten loaded items on each side enter the prefetch window. Near the loaded tail, the main list requests the next result page so the lower buffer can fill. The quick panel uses intersecting row bounds, or its nine displayed image cells, with the same buffer.

The selected detail has first priority, followed by visible entries and then buffer entries. At most four content reads run concurrently. Repeated frames reuse cached content and in-flight reads. Selecting a ready entry displays its cached detail in the same frame. An image first displays its cached thumbnail while a separate read upgrades the selected preview to original resolution. An initial read, fast scrolling ahead of prefetch, or unavailable content can still show the existing loading/error state.

Leaving the window cancels obsolete reads and clears payload references from both the cache and entry projections. The currently selected detail retains one additional payload while its row is outside the window. Switching selection releases that exception. Canceled reads retain their concurrency slot until completion; late results cannot repopulate an obsolete window or query. Changed history revisions invalidate affected details; unchanged background reconciliation preserves them. Neighbor images retain thumbnails with an edge of at most 256 pixels. A 32 MiB conservative reservation for thumbnail CPU pixels, mip levels and encoded reconstruction data determines a smaller power-of-two edge for larger windows. Original oriented dimensions remain available for labels and actual-size geometry. Only the selected image retains an original-resolution preview, bounded independently by 32 MiB encoded data and 16 million pixels. Changing selection downgrades the previous full preview; stale tier completions cannot replace the current tier. Stored and copied content stays unchanged.

`live_images.go` owns one shared bitmap lifecycle across the cache, projections and retained selection. The GUI list, grid, detail pane and Quick Look share the Quick Panel's bitmap paint callback. It schedules mygo `Bitmap.ReleasePixels` after an accelerated paint. A preview drawn in software keeps its decoded pixels while the view owns it. The bitmap retains its encoded reconstruction data for lost GPU textures. Undrawn thumbnails keep their small CPU raster. No recurring rendering timer is added. Each UI process owns a cancellation-aware image request/decode slot through response close, decoding, scaling and re-encoding; the four-content-read limit still applies. The independent GUI and quick-panel processes reuse this code with separate queues. Existing per-entry text and manifest bounds remain in effect. Failed previews stay cached for the current window and can be retried after leaving and reentering it. This cache is local to the desktop session and does not change Engine history or transfer ownership.

Run `make desktop-e2e CASE=search-contract` for real-service viewport scheduling, immediate cached selection, eviction, reverse scrolling, query replacement and quick-panel process evidence. This process scenario supplies viewport indices directly and does not establish native scrolling or visual timing. Run `make desktop-e2e CASE=image-previews` for real-service thumbnail/selection, original-byte, EXIF and serial image-read acceptance. See the [prefetch receipt](../exec/history-detail-prefetch.md) and [memory receipt](../exec/desktop-memory.md). Normal GUI close terminates its process. The independent quick panel drops its windows and history binding between activations. After window teardown, it clears the cancelled history binding and projected rows from the old view, so retained window callbacks cannot keep those history payloads alive. Historical selected-original input allocation and native costs are recorded in the [quick-panel receipt](../exec/quick-panel-memory.md).

## GPUI quick panel fixture

The dedicated `unic-desktop-demo --page quick-panel` interaction/window composition has historical lineage in cached `origin/main` commit `885546c7cf6f6f0d2076104efd1d90d9f73d9744`.
Its current content palette, typography and glyphs use the same native BoardUI resources as the main interface; the old GPUI palette is not a selectable visual scheme.
`desktopdemo/fixtures` owns synthetic quick-panel data. `desktopui/quick_panel_model.go` owns shared quick-panel state and isolated preview effects.
`quick_panel_view.go` owns the 380 × 366 history composition, filter suggestions, nine result shortcuts, 3 × 3 image grid, empty states and action list.
`quick_panel_image.go` owns image fit, actual-size and bounded pan.
`quick_panel_window.go` creates the frameless history window and a parent-owned nonfocusable mygo Flyout.
The preview is 400 points wide; actions use the same satellite at 380 points wide.
The satellite follows the active result with an 8-point gap and delegates edge flipping to mygo.
Search waits 300 ms. The first preview waits 500 ms; subsequent previews wait 120 ms.
Keyboard selection clamps at list boundaries. Hover changes the active preview without changing the keyboard selection.
The history input handles panel commands through mygo OnTextKey, preserving input-method composition and normal copying of selected query text.
Actions replace preview content and retain keyboard handling in the history window. Their cursor wraps over enabled rows.
The fixture accepts filters explicitly, combines values with OR within a dimension and AND across dimensions, and offers independent relaxation proposals after empty searches.
The standalone fixture never creates an Engine client and records restore/paste/send/open/reveal as simulated effects. The normal Desktop uses the same composition through a separate live-data binding and its native host dispatch; fixture success does not establish those real integrations.
The standalone entry can start in history, disconnected or empty state with `--quick-state`.
The daemon-backed Desktop shares the renderer through its live binding. The independently launched demo keeps its isolated fixture data and effects.
The standalone demo uses isolated fixtures. The production quick-panel role reuses the live history binding from `NewDesktop` and creates its own history window and satellite. It delegates clipboard application to the active adapter, through unicd on macOS and through the tray on other platforms.
The [quick panel acceptance guide](../../app/desktop/QUICK-PANEL-DEMO.md) records the source map, reproduction steps, artifact format and differences.
The explicit `quick-panel-screenshots` export composes separately rendered history and satellite rasters at 2x scale and records source and image hashes.
View-level E2E verifies mygo input and layout without opening a native app. Native focus, multi-monitor placement, icon/font rendering and one-to-one visual acceptance remain unverified.

## 缺口

- The adopted demo supplies desktop composition and interaction. Native visual acceptance and the remaining live capabilities are recorded in the [execution receipt](../exec/desktop-live-data.md).
- Mobile physical-device synchronization, system credential prompts and Store rollout have separate acceptance gates. See the [mobile completion plan](../exec/0031-mobile-completion-plan.md) and [release architecture](releases.md#mobile-candidates).

## Independent release programs

The current service application is `app/service/`. Its `unic` and `unicd` programs have no UI dependency.
The desktop application is `app/desktop/`, with the `unic-desktop` program and native clipboard integration.
Shared local protocol, configuration, paths and snapshot conversion live in root `internal/local*` packages.
`internal/clipboardintegration` contains a UI-independent host state machine; Engine does not import it.
The tray adopts a same-profile standalone service without taking ownership, or creates a desktop-owned service through a parent pipe. Manual launch always opens the GUI. Normal GUI close exits only the GUI process and retains the tray and service. Lightweight GUI close exits GUI, tray and quick panel, leaving only unicd without a hotkey or adapter. Explicit background entry keeps the tray, service and windowless quick panel in normal mode and only unicd in lightweight mode. Normal login uses `desktop.start_in_background` to choose whether to open the GUI. Lightweight login keeps only unicd. The preference never suppresses manual activation. Fresh or pending setup always opens the GUI first. Quick-panel dismissal returns its separate process to windowless standby while tray integration remains. Explicit application Quit closes Desktop roles and stops only its owned service. Lightweight close transfers an owned service to standalone through the strict owner-pipe release plus EOF protocol and confirms the same generation and PID through the existing service API without restarting the service.
`app/desktop/internal/lifecycle` owns service acquisition and login intent; `internal/servicelifecycle` owns generation-scoped stop and owner EOF handling. `app/desktop/internal/desktopprocess` owns profile-scoped role locks and private IPC with OS peer executable checks. The [process design](../exec/desktop-process-design.md) records role lifetimes and failure policy.
CLI command requirements live in `app/service/cmd/unic/requirements.go`. The CLI acquires a generation-bound usage lease lazily, reuses a running service, or launches the sibling `unicd --cli-temporary`. The last usage release retires a CLI-temporary service.
`internal/servicelaunch` provides shared launch primitives and the profile transition lock. Desktop and CLI launch policy remain in their respective applications.
`unic send --to all` or `--to DEVICE` commits and sends to paired devices. Without `--to`, `unic send` creates a temporary code/link transfer. `unic recv` accepts a code, address or link. Device management lives under `unic device`; development commands live under `unic dev`.
`app/service/internal/host/transfer.go` owns temporary sessions through `internal/tcnet` and `internal/webtransfer`. Engine admits received transfers through `engine/transfer.go`, then uses the receive pipeline without storing a device or channel.
`internal/observability` exports service telemetry, including tailcat session diagnostics through `internal/syncdiag`; the desktop adapter forwards no analytics identifier.
The development archive builder reuses identical headless binaries in the optional desktop archive. CLI product metadata, native build/release workflows and npm promotion are implemented. Desktop/mobile release paths remain open. See the [release architecture](releases.md).
See [headless service and desktop integration](headless.md) for the process and distribution contract.

Search condition chips can shrink within the available field width. Their values stay on one line and expose the full value in a tooltip. Their remove buttons retain a fixed hit target, including with long tag names.

## Desktop acceptance

The canonical Desktop acceptance entry is `make desktop-e2e`. It runs the declarative catalogue under `app/desktop/e2e`, including native-window evidence, production-process contracts and all migrated software-view scenarios. Original Desktop test entries and standalone scripts are removed. CI uses the same command under Xvfb and retains its report. The running-window captures establish content rendering and app-scoped interaction; OS focus and cross-application paste remain separate. See the [Desktop E2E contract](../contracts/desktop-e2e-contract.md).

### Tag controls and colors

Desktop lists Text, Images, Files, Folders, Code, Links, Email addresses, Color, Version number, UUID and IPv4 address as protected built-in tags, including in an empty library. The labels use the selected interface language. Search, history, assignment and cleanup use stable built-in tag IDs. Text and Links default to blue, Images to green, Files to orange and Code to purple. The other built-in tags default to gray. Text, Images and Files appear in the sidebar by default, including in an empty library. The tag manager saves sidebar visibility for built-in and custom tags. Migration fills unset visibility for tags present in the v1 layout or tag catalogue. Persisted color overrides apply to built-in and custom tags, including v1 migration colors. Built-in tags cannot be manually assigned, removed, renamed, merged or recolored in the manager. Custom tags retain their literal labels. The [pipeline](pipeline.md#built-in-content-tags) defines automatic classification and the no-backfill boundary.

The detail view uses BoardUI-backed application tag chips and an inline assignment composition. Add Tag or T opens the native editor. Suggestions put Create first for a new nonempty name, followed by unattached matching tags or frequent tags. Names are trimmed, limited to 64 Unicode scalars and matched without case for duplicate prevention. Built-in names/IDs cannot become new custom tags. Creation colors remain orange, blue, green, purple and gray. Clicking a swatch keeps editor focus. Arrows select, Return attaches, Tab/Shift+Tab cycle creation colors and Escape cancels. Changing focus or the selected record cancels the editor. Removing a custom chip detaches the tag from that record and keeps the registry.

The management modal uses a compact task Sheet with a separated controls bar (search, sort, inline creation bar with color picker) and a grouped list separating My Tags from protected System Types, with clickable inline color pickers, hover/selection quick actions, sidebar toggles and in-window context actions. Custom color changes update the shared sidebar/search/list/detail registry. Chip background/ink use canonical BoardUI status/chip resources, with explicit caller-owned custom colors. Production supports filtering, sorting, Show Entries, sidebar visibility and color changes. Standalone empty-tag creation and global rename/merge/deletion remain preview operations until the service supports them; production hides/disables those actions. Sidebar visibility survives service refresh in the current Desktop session.

Custom color overrides are local desktop presentation settings in `desktop.tag_colors`, saved through the existing service configuration API and revision check. Keys are lowercase SHA-256 digests of tag IDs, so no raw tag names are added to configuration. Values accept the five palette names or `#rrggbb`; the map is limited to 4096 overrides. New tag creation saves the color configuration before revision-checked history assignment. These writes do not share a transaction: a failed history assignment can leave an unused color override, while the UI reports failure and reconciles authoritative history. Recoloring publishes no optimistic color before a successful save. Refresh and desktop reopen reload saved colors. Invalid configuration preserves the last valid settings; stale saves preserve concurrent settings and report failure. Overrides do not sync between devices. See the [execution receipt](../exec/tag-controls-v1-parity.md) for evidence and native acceptance limits.

Run `make desktop-e2e CASE=production-boundary` to build both desktop commands and verify production excludes demo dependencies. See the [module separation receipt](../exec/desktop-module-separation.md).

## Directory content

Desktop and CLI share the Engine-owned [directory content model](../contracts/directory-contract.md). Directory automatic sync defaults off. Desktop exposes explicit history-to-device send with progress and cancellation. Directory references, snapshots, storage ownership and native acceptance limits are described in [pipeline](pipeline.md) and [desktop runtime](desktop-runtime.md).

## Expo mobile client and in-process Engine

`app/expo` is a single Expo 57 application for Android and iOS. It retains the
imported UniClip presentation at `ac90f94805a9f2003f6e319aa84484cf0bd615c9` with
MIT attribution and platform-resolved iOS SwiftUI/Liquid Glass and Android
Compose/Material components.

In native builds (`modules/unic-engine`), the app embeds the root Go Engine
in-process via `bindings/mobile` (`UnicCore.xcframework` on iOS; unpacked `classes.jar`
+ `jni/libgojni.so` from `unic-engine.aar` on Android). `src/engine/` drives live
setup/v1 migration (`MigrationSession`), history queries/previews, pairing code
and ticket approval with mandatory fingerprint review, device sync management,
actions and streaming file commits. When the native `UnicEngine` module is not
loaded (JS-only/shell runs), `useEngineStore` explicitly enters `mode: "unavailable"`
and retains the in-memory `src/shell` presentation with explicit unavailable notices.

Live settings use the Engine's revision-checked settings API. The mobile
binding saves user-facing Engine settings in `mobile-settings.json` and manual
self-hosted relay keys separately in owner-only `mobile-relay-secret` (mode
`0600`, target-bound `{"target", "access_key"}` JSON). Each file is replaced
independently via temp-file write/sync/rename; if persistence fails on
`UpdateSettings`, the binding attempts best-effort restoration of the prior
secret file and in-memory Engine settings state (ignoring restore errors). On
startup, `loadRelaySecretForTarget` admits a persisted key only when its
recorded canonical target matches the loaded `sync.relay.url`; unattributed
legacy records, target mismatches, or malformed records are rejected and
attempted to be removed (healing `has_access_key`) before Engine starts. The
binding also manages UniClipboard Cloud PKCE session exchange,
`cloud-session.json` persistence, scoped relay device credential rotation,
HTTPS-only advertised `relay_url` binding, non-blocking admission via
`engine.Options.RelayAuth`, and authoritative revocation inside the in-process Go
runtime shared with headless background services and the iOS Keyboard Extension.
Mobile browser sign-in uses Expo Crypto's native random generator for the PKCE
verifier and OAuth state, and its native SHA-256 digest for the S256 challenge.
On iOS a callback received in the background waits up to ten seconds for the
host to resume the Engine in the foreground. The host keeps the grant in
memory, exchanges it once after resumption, and discards it on timeout,
shutdown, or resumption failure. The normal background release frees the
shared profile for the Keyboard Extension.
The JS login flow hands only temporary authorization parameters to the
Go binding and receives secret-free status snapshots without retaining tokens or
relay keys in JS storage, while official relay settings hide manual key
controls and self-hosted URLs keep write-only key entry (separate from Cloud web
`/account/api-keys` and `/account/devices` acceptance). Emulator/simulator UI
and cold-start checks over preset interrupted disk states pass, whereas live
mid-write `SIGKILL` crash injection, exhaustive filesystem error recovery,
native browser PKCE callback redirects, signed App Group / Keyboard Extension
handoff, Android background renewal, and physical-device two-peer sync remain
separate unverified boundaries. The host stores automatic clipboard detection
and automatic application separately. Android observes clipboard changes only
while the app can read the clipboard. iOS offers an explicit read after
detecting a change. Storage settings report Engine file-cache usage and clear
only the host's image previews.

The history views obtain file names through short-lived content leases. The
native hosts import camera and photo selections as PNG representations. The
photo and document exporters stream the saved parts with their original bytes.
System sharing uses native share sheets. Directory details show up to 200
entries from the stored snapshot or the direct children of a live reference.

The client coalesces Engine events by kind. History, device and status reads
run independently. Startup, foreground reconciliation and resync refresh all
collections. Image previews use a private cache limited to 120 files and
100 MiB. Reconciliation removes previews for deleted or expired history.
See the [mobile settings and media receipt](../exec/0031-mobile-m2-receipt.md).

The device screen reads pairing operations on Engine operation events,
resync, and foreground reconciliation. Both peers compare the shared
fingerprint before approval. The local device row displays the Engine
device ID. The ticket view clears on close, expiry and backgrounding.
History tags use revision-checked updates and Engine tag queries.

Android SAF copies the selected v1 backup to private cache before passing
it to the existing migration session. Android share intents and the iOS
Share Extension stage items for import in the main application.
The iOS extension uses the file URL to recover the file name and type when
the share provider offers the payload as a file URL.
After staging succeeds, the iOS extension requests a handoff using the host
bundle identifier as the URL scheme and `share` as the URL host. The URL
carries no shared content. The main application reads the App Group queue
on mount, on receipt of the share URL, and when the app becomes active.
If iOS declines the handoff, the staged items remain available when the user
opens the app.
The main application saves staged content to local Engine history without
requesting network dispatch. Once saved, the item opens in the same device
send sheet as the history action. Dismissing the sheet keeps the history
record. Both entry points require an explicit device selection, and sending
reuses the stored record. The sheet reads transfer status from the Engine.
The Android foreground service keeps the app-owned Engine active
while the service runs. Clipboard observation still requires the foreground
conditions reported by the host.

The mobile binding build trims Go source paths and omits Go symbol tables and
DWARF data from the iOS XCFramework and Android AAR. Native packaging retains
its own debug symbol settings. Android x86-64 uses an NDK-built SQLite driver
with FTS5 because the Go-translated Linux driver invokes a disallowed syscall
on that ABI. The binding build enables FTS5 with the `sqlite_fts5` tag. The
store and backup reader retain the same schema and connection policies.

The default Expo build keeps its isolated shell identity. The production
variant preserves the previous mobile package identities and resolves each
platform's version independently from `release/products.toml`. The mobile
binding owns its observability controller and the profile's upload switch.
See [release construction](releases.md) and [observability](observability.md).

Generated native directories belong under `app/expo/ios` and `app/expo/android`
and are ignored. `app/ios` and `app/android` are not build inputs. `npm run verify`
checks source boundaries, TypeScript, pinned navigation patches, Expo prebuild
and both Metro/Hermes bundles. The native live suite covers first install,
history and search, the device page, and diagnostics. Store candidate
construction and physical-device sync soak have separate acceptance evidence.
See [the UI shell receipt](../exec/expo-ui-shell.md),
[the Engine integration receipt](../exec/0031-mobile-engine-integration.md),
and [the mobile release preparation receipt](../exec/0031-mobile-m5-receipt.md).


### Resuming a later Desktop migration

The Settings migration action stops and waits for the owned adapter and service,
then persists pending setup before displaying migration. Cancelling or quitting
preserves that intent. Reopening the same data directory resumes setup rather
than starting history implicitly. Explicit history entry publishes Desktop setup
completion. This navigation receipt is separate from the CLI's explicit
[migration initialization](../contracts/local-service-contract.md#explicit-cli-migration-initialization).

<a id="native-boardui-first-slice"></a>

## Native BoardUI presentation

`app/desktop/boardui` is the reusable native mygo control/resource package. `app/desktop/internal/desktopkit` is the application-only composition layer. Production main/setup/Quick Panel, the full Desktop demo and the separate bounded component demo use this single presentation. Core Engine and UI-free CLI/service dependency graphs remain UI-free.

The fixed visual source is official MIT BoardUI export `ec5c5eb3e29c2441be63b9422cd54f411ef23466`. The current [source lock](../../app/desktop/boardui/assets/source-lock.json) records 111 source-resolved semantic/accent colors per scheme, 52 composite type styles, source geometry/motion and the pinned Inter font. The application uses a bounded 51-glyph functional Remix set with its [own source lock](../../app/desktop/internal/desktopkit/icons-source.json) and independent notice. The public package retains its separate small helper-glyph set. Generated resources have no web runtime dependency. Code-defined immutable light/dark themes use up to twelve base colors per scheme through `boardui.NewPaletteTheme`; component states derive from those bases, while `WithOverrides` applies exact final token replacements. Empty palettes preserve source colors. Public controls, `desktopkit` compositions and native `ui.Theme` share the resolved colors. The [theme guide](../../app/desktop/boardui/README.md#code-defined-color-themes) documents host injection and the complete [color catalogue](../../app/desktop/boardui/COLOR-TOKENS.md). Default source resources remain unchanged.

The public package exposes the implemented native controls and caller-owned state described by the [component contract](../contracts/boardui-native-contract.md). Product lists, navigation, search, tags, schema settings and sheets remain explicit application compositions; they do not imply that every free BoardUI item or a Pro template has been ported. The old skin solution and generator are not a fallback.

Canonical Desktop cases exercise the shared renderer with isolated profiles and synthetic data. The separate actual-React replay compares specific component scenes. Their reports distinguish source/build checks, software geometry/color results, production service behavior and native-platform evidence. Native OS keyboard/IME, platform accessibility, compositor/material output, multi-monitor focus/paste and Windows resource behavior require their own executed evidence. User visual approval remains separate. See the [package guide](../../app/desktop/boardui/README.md), [component receipt](../exec/boardui-mygo-implementation.md) and [production migration receipt](../exec/boardui-production-migration.md).
