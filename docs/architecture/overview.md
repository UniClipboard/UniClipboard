---
paths:
  - go.mod
  - engine
  - internal
  - plugin
  - plugins
  - app/desktop
  - app/service
  - app/android
  - app/ios
  - app/mac/Package.swift
  - app/mac/Sources
  - .github/workflows
  - e2e/cli
  - tools/docscheck
verified: e697ae2bb212684e36d079877fccc79833dcd75c
---
# 仓库布局与依赖边界

unic 使用 monorepo：平台无关的 Go 核心位于根目录，各端宿主位于 `app/<平台>/`，由宿主装配核心并负责系统集成。

## 现状

- 根 `go.mod` 的模块路径为 `github.com/uniclipboard/unic`。当前只有一个 Go 模块，没有 `go.work` 或 `bindings/`；`go test -race ./...` 覆盖核心、插件和桌面端的 Go 测试。
- `engine/` 是公开 Go SDK，管理生命周期、设备信任、历史、处理流水线与同步调度；根 `internal/` 放核心实现及内置插件。`plugin/` 提供独立进程插件 SDK、RPC 和设置 Schema 工具，`plugins/` 放插件示例及开发说明。
- Root `internal/v1migration/` provides old-history migration through the in-process `unic migrate` command. It includes desktop profile discovery and platform credential reads, such as macOS Security.framework. Engine does not import it. There is no separate migration program.
- `app/service/` contains the UI-free CLI and service. `app/desktop/` contains `unic-desktop`, mygo, UI Kit and native integration. Shared local protocol, configuration, paths and content conversion live in root `internal/local*` packages.
- `engine.Settings.Validate` validates Engine business settings. `internal/localconfig.Config.Validate` validates host settings and calls it. Engine does not parse TOML or watch configuration files.
- `app/mac/` 是独立的 Swift Package，保留 SwiftUI 界面与插件设置表单的实验代码，尚未连接 daemon；该客户端路线已取消，停止推进服务接入，现有实验画面不作为最终界面规格。`app/android/`、`app/ios/` 目前仅有各端 README 与 AGENTS.md，没有移动端工程。每个现有应用目录都有独立的 README 与 AGENTS.md。
- CI runs lint, formatting, race checks, all three entry-point builds, and production headless process acceptance. It does not build Swift or mobile applications.

## 不变量

- 应用依赖核心，根 `engine/`、`internal/`、`plugin/` 不得依赖任何 `app/*`。
- Desktop-specific Go packages live in `app/desktop/internal/`. Go internal visibility prevents direct imports by other applications or future bindings. Cross-platform capabilities use the public SDK or protocol. The existing migration exception is root `internal/v1migration/`.
- `engine` 的传递依赖不得包含应用包、mygo、fsnotify、TOML 解析器、tailcat 会话层 `internal/session` 或传输层 `internal/tcnet`。`engine/boundary_test.go` 检查这条边界；内置后端由宿主显式装配。
- 系统剪贴板、托盘、GUI、本机 HTTP/socket 服务和配置文件管理属于宿主；核心只接收类型化设置和宿主通过 SDK 提交的观察、应用结果。
- 当前保持单一根 Go 模块。等移动端实际引入 gomobile 时，再将 `bindings/` 建为嵌套模块并添加本地开发用 `go.work`；不提前创建空绑定层。
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
| `app/android/`、`app/ios/` | 移动端接入约束与说明 |
| `e2e/cli/`、`tools/docscheck/` | CLI 多进程端到端验证与文档检查 |
| `e2e/real/` | Node-configured real-machine CLI testing over local processes and SSH; see [architecture](real-machine-testing.md) |

## 规格与背景

- SDK 边界：[Engine 公共契约](../contracts/engine-contract.md)、[SDK 扩展类型契约](../contracts/sdk-extension-contract.md)。
- 宿主装配：[后端迁移契约](../contracts/backend-migration-contract.md)。
- 业务地图：[流水线与本机历史](pipeline.md)、[设备身份、信任与配对](identity-and-trust.md)。
- 应用说明：[桌面](../../app/desktop/README.md)、[macOS 预览](../../app/mac/README.md)、[Android](../../app/android/README.md)、[iOS](../../app/ios/README.md)。

## Desktop layout demo

`unic-desktop --demo` opens the native desktop layouts with in-memory fixtures.
The composition follows local `main` commit `8dd3796f8bbb6b1e1e07ee352a4e65e4341f56e7`.
`app/desktop/internal/desktopdemo` owns demo state and page composition.
The existing UI Kit owns Adwaita, macOS glass, and Fluent styling.
The macOS demo uses a unified native title bar, a 1100 x 720 default window, toolbar glass capsules, and an inset floating detail action bar.
The Kit exposes a toolbar mode for segmented controls and a secondary Glass button variant; other skins retain their native styling.
MacOS capsule surfaces share mygo plugins/glass.Glass (Regular, Interactive). The renderer supplies refraction, reactive rim lighting and press growth. The shader samples this native Go UI frame; direct NSGlassEffectView composition is not used for these controls.
MacOS buttons and segments share physical press feedback: an 80 ms compression and a 350 ms spring return. Glass buttons retain the material's press expansion. The toolbar selection thumb moves continuously with retargetable spring motion. Reduce Motion suppresses these spatial effects; pointer selection does not paint the keyboard-only segment focus ring.
MacOS Select, uncontrolled Menu, and uncontrolled ContextMenu use mygo's system menu API. Kit.Apply installs SystemMenus. On macOS, mygo builds NSMenu and lets the OS own material, layout, highlighting, tracking, and presentation animation. These menus open on pointer-down and do not use the Kit's popup animation. Explicit Open pointers retain the caller-controlled in-window overlay contract. Menu entries share the existing list-menu adapter and cached glyph/swatch images. Select shows its focus ring only for keyboard focus.
System-menu triggers align to the center of their parent row, matching adjacent toolbar command buttons without a MenuBase wrapper. The history toolbar and detail action bar share a send-target menu projected from the current remote-device registry, with All Devices and individual peers, excluding the local device. The detail trigger retains its large Glass button style. Selection records a simulated send result; opening and cancellation do not send.
The history demo composes Kit.TokenSearchField with its toolbar disclosure variant and a separate demo-owned search model (desktopdemo/search_model.go). The model handles @ devices, # tags, / types, on: time and ext: extensions; candidates, counts, chips, keyboard completion, literal fallback, and caret-preserving edits share the existing filter state. MacOS search expands from a 32-point icon to a 460-point field over 280 ms. Escape closes candidates, clears search, then collapses an empty field. The title bar, traffic lights, sidebar and command capsule keep their existing implementation; no AppKit toolbar replacement is installed. Other skins use their normal token field. Reduce Motion follows the renderer preference.
Search badge removal resets the whole filter dimension. Backspace in an empty input deletes the entire last badge, including all tags grouped in it; it does not reopen the badge text for editing. Other conditions remain active.
The history demo tag registry is shared by sidebar visibility, search candidates, list badges, and detail badges. Its management sheet disables column resizing and reordering through TableProps.FixedColumns, which uses mygo TableColumn.Fixed. Other tables keep their existing drag behavior. The sheet provides a table, sorting, filtering, inline creation/renaming, colors, merging, and deletion. Builtin tags allow Show Entries but reject destructive edits. Table context menus use the existing platform menu adapter (NSMenu on macOS). Deletion and merging preserve history entries. The detail assignment picker remains separate. Tag metadata and associations stay in memory. Kit tables with a filling column retain a flexible trailing column after resizing would otherwise make all columns fixed, preventing uncovered trailing space while keeping the dragged divider responsive. These tables bound column growth to the viewport and reserve space for the remaining columns, so dragging cannot push trailing controls outside the table.
The animated sidebar viewport has no padding. A fixed-width inner body owns the padding, so the viewport can reach zero without a final width jump.
The shared macOS Sheet uses a 300 ms downward entry and a 220 ms upward exit with a separately fading scrim. DialogBase continues to own input scoping and focus restoration. Sheet retains the original live dialog and modal scope until exit completes, then restores opener focus. The panel keeps its opacity and rendering path while the scrim fades separately. Callers keep invoking Sheet while closed and clear transient content state in OnClosed after exit. Virtual tables are not copied or relaid out through an exit snapshot. Reduce Motion suppresses panel translation. Other skins retain their existing dialog behavior.
The demo includes history list/grid/detail, a device management sheet and nine settings categories. Setup, space creation, unlock and recovery-preview flows are removed. Settings opens the dedicated quick panel window.
The device sheet uses a single pane below the Kit breakpoint, with explicit list/detail navigation. Pending-device details show the fingerprint and direct accept/reject actions. These actions change fixtures only.
Below 1100 px, the sidebar opens as an overlay. Below 760 px, history list and detail share one pane.
Desktop pages and Kit components construct standard vertical scroll containers through `Kit.Scroll`; direct `ui.Scroll` construction is confined to that factory. Lists and grids reuse its indicator policy.
Detail, nested previews, settings, sheets, and navigation follow the same rule: show after position changes and hide after 600 ms without movement; hover alone does not show an indicator. Each container keeps independent state. History list and grid content also reserve a scrollbar gutter.
The demo does not create an API client, write the clipboard, register global shortcuts, or persist settings.
`unic-desktop` continues to use the daemon-backed interface.
Build checks do not establish native layout or interaction acceptance.
See the [demo acceptance guide](../../app/desktop/UI-DEMO.md) for source mappings and manual checks.

## Live desktop composition

The normal desktop opens the shared history composition directly. It has no setup or unlock gate.
`desktopdemo.NewDesktop` initializes empty service-backed state, without fixture data.
`live.go` serializes local socket work and applies completions on the view thread.
It owns query pagination, reconciliation, bounded content previews and revision-checked history mutations.
The event stream requests fresh snapshots; a five-second reconciliation also covers lost events.
Refresh preserves loaded page depth. Query changes discard results for obsolete filters.
`live_devices.go` binds existing pairing, approval, candidate and removal operations.
`live_settings.go` binds the existing General, Sync and Storage settings; the launcher owns native login registration.
The service lifecycle, exclusive clipboard adapter, tray and normal/lightweight modes remain defined by the [headless architecture](headless.md).
Global literal text/file-name search, rich-text classification, extension/source/tag/time filters, paged candidates and exact prospective counts use the Engine-owned [search interface](../contracts/history-search-contract.md).
The main editor keeps word/all-tag semantics; the quick panel keeps phrase/any-tag semantics. Both reject obsolete read generations and use separately cancellable search/count/content reads.
The normal desktop opens a real-data quick-panel window from Settings or the history menu. Copy/delete call the existing service; automatic OS paste, semantic classifiers, favorites, full tag management, richer device policies/status and direct history sending remain gaps.
The normal UI never falls back to fixture data or simulated effects.
See the [execution receipt](../exec/desktop-live-data.md) for nonvisual process acceptance and native limits.

## GPUI quick panel fixture

The dedicated `unic-desktop --demo --page quick-panel` entry follows cached `origin/main` commit `885546c7cf6f6f0d2076104efd1d90d9f73d9744`.
It replaces the earlier generic history/detail quick-panel composition.
`desktopdemo/quick_panel_model.go` owns fixture state and simulated effects.
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
The standalone fixture never creates an Engine client. The normal desktop uses the same composition through a separate real-data binding. Entry restore, paste, send, open and reveal remain recorded simulated effects.
The standalone entry can start in history, disconnected or empty state with `--quick-state`.
The regular daemon-backed desktop shares the main composition through `NewDesktop`; the quick panel remains an isolated fixture.
The [quick panel acceptance guide](../../app/desktop/QUICK-PANEL-DEMO.md) records the source map, reproduction steps, artifact format and differences.
The explicit `TestQuickPanelScreenshotsE2E` export composes separately rendered history and satellite rasters at 2x scale and records source and image hashes.
View-level E2E verifies mygo input and layout without opening a native app. Native focus, multi-monitor placement, icon/font rendering and one-to-one visual acceptance remain unverified.

## 缺口

- The adopted demo supplies desktop composition and interaction. Native visual acceptance and the remaining live capabilities are recorded in the [execution receipt](../exec/desktop-live-data.md).
- Android、iOS 工程及移动绑定尚未建立；gomobile 接入时需补嵌套模块、工作区和版本标签约定（`bindings/vX.Y.Z`）。
- 各端原生 CI 尚未建立。随应用工程接入，再实现核心变更验证所有端、应用变更验证对应端的路径分流。

## Independent release programs

The current service application is `app/service/`. Its `unic` and `unicd` programs have no UI dependency.
The desktop application is `app/desktop/`, with the `unic-desktop` program and native clipboard integration.
Shared local protocol, configuration, paths and snapshot conversion live in root `internal/local*` packages.
`internal/clipboardintegration` contains a UI-independent host state machine; Engine does not import it.
The normal desktop owns a profile-scoped service through a parent pipe. Lightweight mode leaves a standalone service.
`app/desktop/internal/lifecycle` owns replacement and login intent; `internal/servicelifecycle` owns generation-scoped stop and owner EOF handling.
CLI command requirements live in `app/service/cmd/unic/requirements.go`. The CLI acquires a generation-bound usage lease lazily, reuses a running service, or launches the sibling `unicd --cli-temporary`. The last usage release retires a CLI-temporary service.
`internal/servicelaunch` provides shared launch primitives and the profile transition lock. Desktop and CLI launch policy remain in their respective applications.
`unic send --to all` or `--to DEVICE` commits and sends to paired devices. Without `--to`, `unic send` creates a temporary code/link transfer. `unic recv` accepts a code, address or link. Device management lives under `unic device`; development commands live under `unic dev`.
`app/service/internal/host/transfer.go` owns temporary sessions through `internal/tcnet` and `internal/webtransfer`. Engine admits received transfers through `engine/transfer.go`, then uses the receive pipeline without storing a device or channel.
`internal/observability` exports service telemetry, including tailcat session diagnostics through `internal/syncdiag`; the desktop adapter forwards no analytics identifier.
The archive builder reuses identical headless binaries in the optional desktop archive. Independent product version/release workflows remain planned in decision 0018.
See [headless service and desktop integration](headless.md) for the process and distribution contract.
