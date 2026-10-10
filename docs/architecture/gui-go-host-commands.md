# Go 宿主命令契约

桌面宿主（`apps/gui-go`）向共享前端（`apps/gui-go/frontend/src`）暴露的全部命令与事件，其唯一契约来源是 Go 源码：`*HostService` 的导出方法签名、`apps/gui-go/host_types.go` 的 DTO 与 `apps/gui-go/host_events.go` 的事件注册。前端的调用函数和数据类型由 Wails 官方生成器产出，不存在第二份手写 schema。

```text
Go 方法 / DTO / RegisterEvent
        │  wails3 generate bindings（官方生成器）
        ▼
apps/gui-go/frontend/bindings/…/{hostservice,models,index}.ts   ← 已提交，CI 重新生成并 diff
        │
        ▼
apps/gui-go/frontend/src/lib/ipc.ts  commands：按 camelCase 重新命名，并统一加 trace、面包屑、脱敏与 Sentry 分级
```

## 生成与校验

| 目的 | 命令 |
| --- | --- |
| 重新生成 Wails 绑定与错误目录的前端形态 | `bun run gen:host-contract` |
| 校验已提交的生成物未漂移（CI） | `bun run check:host-contract` |
| 校验 `//uc:` 指令、错误码目录、事件常量等源码规则 | `go run ./cmd/hostcontract lint`（在 `apps/gui-go` 下） |
| 重新生成本文档的生成区块 / E2E 控制表 | `go run ./cmd/hostcontract docs`、`go run ./cmd/hostcontract e2e-table`（均支持 `-check`） |
| 校验事件的监听方与发射方互相对应 | `node scripts/architecture/check-host-events.mjs` |

Wails 版本与 Go 工具链只在 `apps/gui-go/go.mod` 中固定一处，生成脚本从中读取。这些检查是静态门禁：它们证明契约没有漂移，不证明运行时行为；运行时行为只由真实 E2E 证明（`apps/gui-go/e2e/`）。

## 调用语义（Wails 框架行为，已在源码与 E2E 中验证）

- 参数按位置传递。参数个数不符时前端得到 `TypeError`；`undefined` 与 `null` 都会解码为非指针参数的零值，所以“缺失参数”与“空值”不可区分。必须区分的参数使用指针类型，并在方法内显式校验。
- 方法首参为 `context.Context` 时由框架注入，前端取消调用即取消该 context；超时由 `commandContext` 补上（默认 30 秒，`download_update` 与 `install_update` 为 30 分钟）。
  `download_update` 是例外：下载在后台独立于调用的 context 运行（窗口关闭或重新挂载不应中断下载），显式取消用 `cancel_download`，被取消的调用以文本错误拒绝，待更新回到可下载状态，页面收到 `Failed` 事件。
- 方法返回的 `error` 在前端表现为 `RuntimeError`，其 `.cause` 是 `hostapi.Marshal` 编码的 JSON；panic 被框架恢复为没有 `cause` 的 `RuntimeError`。`apps/gui-go/frontend/src/lib/ipc.ts` 的 `hostRejection` 负责把它还原为类型化错误对象。
- Go 的 `[]byte` 在 JSON 中是 base64 字符串（`save_image_as`、`open_image_externally` 的 `data`），不再是数字数组。
- 命名字符串常量生成为 TypeScript `enum`，成员名等于 Go 常量名（例如 `InstallKind.InstallKindDeb`）；Wails 不生成字面量联合。
- `install_update` 原先通过 Channel 回传进度，现改为带类型的广播事件 `update-install-progress`：页面在调用前订阅，调用结束后取消订阅。该事件只由 `install_update` 产生。

## 错误契约与已知限制

所有跨界的错误码都登记在 `apps/gui-go/internal/hostapi/errors.go` 的 `Catalog` 中，并带有严重度（用户可见的业务失败 / 系统失败）。`hostapi.Marshal` 是服务级 `MarshalError`：目录外的码降级为 `InternalError`，普通 `error` 包装为 `InternalError`。严重度表和错误联合类型由 `hostcontract errors-ts` 导出到 `apps/gui-go/frontend/src/lib/host-errors.generated.ts`。

限制：Wails 绑定的方法只返回 `error`，框架无法为每个命令生成“可能的错误联合”。因此各命令声明的错误族与错误码写在方法注释的 `//uc:errors` 指令里，由 `hostcontract lint` 与方法体实际使用的 `hostapi` 常量对账，并展示在下表中；这是最小可行方案，不声称由框架生成。

业务失败（如口令错误、对话框取消）不上报 Sentry；其余失败上报。`ConfigError` 的 `cancelled` 现在也按业务失败处理（此前会被误报）。

## 命令与事件一览

<!-- BEGIN GENERATED: host-contract -->

此区块由 `go run ./cmd/hostcontract docs` 从 Go 源码生成（命令来自 `*HostService` 的导出方法及其 `//uc:` 指令，事件来自 `apps/gui-go/host_events.go`），请勿手改。

### 命令（66 个）

| 命令 | Go 方法 | 参数 | 结果 | 错误族 / 错误码 | macOS | Windows | Linux | 来源 / 说明 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `begin_visual_effects_sample` | `BeginVisualEffectsSample` | `sessionID string` | `*SamplePermit` | none | 有意空操作 | 有意空操作 | 有意空操作 | 契约命令；BeginVisualEffectsSample asks for permission to measure one sample. |
| `cancel_download` | `CancelDownload` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；CancelDownload cancels a running download. |
| `check_for_update` | `CheckForUpdate` | `channel *string` | `*UpdateMetadata` | text | 真实 | 真实 | 真实 | 契约命令；CheckForUpdate looks for a newer release on the given channel (null = the user's channel) and tells every window through `update-available`. |
| `dev_open_updater_window` | `DevOpenUpdaterWindow` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；DevOpenUpdaterWindow opens the update window in preview mode (`?dev=1`), which shows a sample release. |
| `dismiss_quick_panel` | `DismissQuickPanel` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；DismissQuickPanel hides the quick panel and gives the focus back to the application that had it. |
| `download_update` | `DownloadUpdate` | - | `-` | text | 真实 | 真实 | 真实 | 契约命令；DownloadUpdate downloads the pending release in the background; progress is broadcast on `update-download-progress`. |
| `export_config_package` | `ExportConfigPackage` | - | `ExportConfigResult` | config | 真实 | 真实 | 真实 | 契约命令；ExportConfigPackage asks where to save, then has the daemon write the configuration bundle there. |
| `export_startup_logs` | `ExportStartupLogs` | - | `*string` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；ExportStartupLogs zips the logs to a place the user picks. |
| `finalize_quick_panel_show` | `FinalizeQuickPanelShow` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；FinalizeQuickPanelShow is the second phase of showing the panel, after the page cleared its stale state. |
| `get_auto_download_update` | `GetAutoDownloadUpdate` | - | `bool` | text | 真实 | 真实 | 真实 | 契约命令；GetAutoDownloadUpdate reads the "download updates automatically" setting. |
| `get_content_unlocked` | `GetContentUnlocked` | - | `bool` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；GetContentUnlocked reports whether the daemon lets this GUI show content right now. |
| `get_daemon_bootstrap_failure` | `GetDaemonBootstrapFailure` | - | `*DaemonBootstrapFailure` | none | 有意空操作 | 有意空操作 | 有意空操作 | 契约命令；GetDaemonBootstrapFailure reports why the daemon bootstrap failed. |
| `get_daemon_connection_info` | `GetDaemonConnectionInfo` | - | `*DaemonConnection` | none | 真实 | 真实 | 真实 | 契约命令；GetDaemonConnectionInfo tells the page where the daemon listens. |
| `get_daemon_session` | `GetDaemonSession` | - | `*DaemonSession` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；GetDaemonSession exchanges a short-lived daemon session for the page. |
| `get_daemon_startup_status` | `GetDaemonStartupStatus` | - | `json.RawMessage` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；GetDaemonStartupStatus returns the daemon's startup progress (`GET /startup`) as the daemon sent it: nil while the daemon is not reachable yet. |
| `get_desktop_theme` | `GetDesktopTheme` | - | `DesktopThemeSnapshot` | none | 有意空操作 | 有意空操作 | 不支持 | 契约命令；GetDesktopTheme reports the desktop theme. |
| `get_device_id` | `GetDeviceID` | - | `string` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；GetDeviceID returns this device's peer id. |
| `get_device_meta` | `GetDeviceMeta` | - | `DeviceMeta` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；GetDeviceMeta returns the host's device and application metadata for the page's Sentry scope. |
| `get_download_progress` | `GetDownloadProgress` | - | `DownloadProgressSnapshot` | none | 真实 | 真实 | 真实 | 契约命令；GetDownloadProgress returns the pending update state, so a window that mounts mid-download can catch up before it listens to the broadcast events. |
| `get_install_kind` | `GetInstallKind` | - | `InstallKind` | none | 真实 | 真实 | 真实 | 契约命令；GetInstallKind tells how this copy was installed, so the page can route package-managed copies to their package manager instead of the in-app updater. |
| `get_profile_recovery` | `GetProfileRecovery` | - | `json.RawMessage` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；GetProfileRecovery returns the daemon's profile recovery state (`GET /encryption/recovery`) untouched. |
| `get_quick_panel_double_tap_availability` | `GetQuickPanelDoubleTapAvailability` | - | `ModifierDoubleTapAvailability` | none | 真实 | 真实 | 真实 | 契约命令；GetQuickPanelDoubleTapAvailability tells whether the modifier double tap trigger can work in this session. |
| `get_visual_effects` | `GetVisualEffects` | - | `EffectsSnapshot` | none | 真实 | 真实 | 真实 | 契约命令；GetVisualEffects returns the visual effects state of this session. |
| `host_notification_permission` | `HostNotificationPermission` | - | `bool` | none | 真实 | 真实 | 真实 | 适配器：`@/host/notification`；HostNotificationPermission reports whether system notifications are allowed. |
| `host_notification_request_permission` | `HostNotificationRequestPermission` | - | `NotificationPermission` | none | 真实 | 真实 | 真实 | 适配器：`@/host/notification`；HostNotificationRequestPermission asks the system for notification permission (adapter command). |
| `host_notification_send` | `HostNotificationSend` | `options HostNotification` | `-` | command: InternalError | 真实 | 真实 | 真实 | 适配器：`@/host/notification`；HostNotificationSend shows a system notification (adapter command). |
| `import_config_package` | `ImportConfigPackage` | `password string, sourcePath string` | `ImportConfigStageResult` | config | 真实 | 真实 | 真实 | 契约命令；ImportConfigPackage validates a bundle and stages it to be applied at the next start. |
| `install_update` | `InstallUpdate` | - | `-` | text | 真实 | 真实 | 真实 | 契约命令；InstallUpdate downloads (if needed) and installs the pending release, then relaunches or quits as the platform's installer requires. |
| `main_window_presentation_ready` | `MainWindowPresentationReady` | `generation string` | `-` | none | 有意空操作 | 有意空操作 | 有意空操作 | 契约命令；MainWindowPresentationReady is the page's handshake that the main window finished its first paint. |
| `mark_main_window_ready` | `MarkMainWindowReady` | `generation string` | `-` | none | 有意空操作 | 有意空操作 | 有意空操作 | 契约命令；MarkMainWindowReady is the page's handshake that the main window is ready to be shown. |
| `mark_quick_panel_ready` | `MarkQuickPanelReady` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；MarkQuickPanelReady tells the host the panel page finished loading; a toggle requested earlier then takes effect. |
| `open_data_directory` | `OpenDataDirectory` | - | `-` | command: InternalError, NotFound | 真实 | 真实 | 真实 | 契约命令；OpenDataDirectory reveals the application data folder in the file manager. |
| `open_image_externally` | `OpenImageExternally` | `fileName string, data []byte` | `-` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；OpenImageExternally hands image bytes to the system viewer through a single scratch file that is replaced on every call. |
| `open_logs_directory` | `OpenLogsDirectory` | - | `-` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；OpenLogsDirectory opens the log folder in the file manager, creating it first. |
| `open_updater_window` | `OpenUpdaterWindow` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；OpenUpdaterWindow opens the software update window, or focuses it when it is already open. |
| `open_url` | `OpenURL` | `url string` | `-` | command: InternalError | 真实 | 真实 | 真实 | 适配器：`@/host/opener`；OpenURL opens a link in the default browser. |
| `paste_to_previous_app` | `PasteToPreviousApp` | - | `-` | text | 不支持 | 真实 | 真实 | 契约命令；PasteToPreviousApp hides the panel, returns to the previously focused application and pastes. |
| `pick_config_bundle_path` | `PickConfigBundlePath` | - | `*string` | config | 真实 | 真实 | 真实 | 契约命令；PickConfigBundlePath shows the native open dialog for a configuration bundle. |
| `pick_directory` | `PickDirectory` | - | `*string` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；PickDirectory shows the native folder picker. |
| `preview_config_import` | `PreviewConfigImport` | `password string, sourcePath string` | `ConfigImportPreview` | config | 真实 | 真实 | 真实 | 契约命令；PreviewConfigImport reads the descriptive metadata of a bundle without importing it. |
| `quick_panel_uses_compositor_shortcuts` | `QuickPanelUsesCompositorShortcuts` | - | `bool` | none | 真实 | 真实 | 真实 | 契约命令；QuickPanelUsesCompositorShortcuts reports whether the shortcut is bound by the Wayland compositor instead of the app. |
| `report_visual_effects_environment` | `ReportVisualEffectsEnvironment` | `sessionID string, systemMotion SystemMotion` | `EffectsSnapshot` | command: ValidationError | 真实 | 真实 | 真实 | 契约命令；ReportVisualEffectsEnvironment records the system reduce-motion preference the page observed. |
| `report_visual_effects_sample` | `ReportVisualEffectsSample` | `sample EffectsSample` | `EffectsSnapshot` | none | 有意空操作 | 有意空操作 | 有意空操作 | 契约命令；ReportVisualEffectsSample accepts a frame-timing sample. |
| `resolve_quick_panel_expand_side` | `ResolveQuickPanelExpandSide` | `scale *float64` | `QuickPanelExpandSide` | none | 有意空操作 | 有意空操作 | 有意空操作 | 契约命令；ResolveQuickPanelExpandSide tells which side the inline preview opens toward. |
| `restart_app` | `RestartApp` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；RestartApp restarts the daemon and then the GUI. |
| `restart_daemon` | `RestartDaemon` | - | `-` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；RestartDaemon replaces the daemon process; the page reconnects itself. |
| `reveal_path` | `RevealPath` | `path string` | `-` | command: InternalError, NotFound | 真实 | 真实 | 真实 | 契约命令；RevealPath shows an existing file or folder in the file manager. |
| `save_image_as` | `SaveImageAs` | `fileName string, data []byte` | `*string` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；SaveImageAs writes image bytes to a place the user picks. |
| `set_auto_download_update` | `SetAutoDownloadUpdate` | `enabled bool` | `-` | text | 真实 | 真实 | 真实 | 契约命令；SetAutoDownloadUpdate saves the "download updates automatically" setting. |
| `set_follow_omarchy_theme` | `SetFollowOmarchyTheme` | `enabled bool` | `DesktopThemeSnapshot` | none | 有意空操作 | 有意空操作 | 不支持 | 契约命令；SetFollowOmarchyTheme would switch the page to the Omarchy theme. |
| `set_quick_panel_double_tap_modifier` | `SetQuickPanelDoubleTapModifier` | `modifier QuickPanelDoubleTapModifier` | `-` | command: ValidationError, Conflict, InternalError | 真实 | 真实 | 真实 | 契约命令；SetQuickPanelDoubleTapModifier sets the modifier whose double tap opens the quick panel. |
| `set_quick_panel_enabled` | `SetQuickPanelEnabled` | `enabled bool` | `-` | command: InternalError, Conflict | 真实 | 真实 | 真实 | 契约命令；SetQuickPanelEnabled turns the quick panel and its global shortcut on or off. |
| `set_quick_panel_layout` | `SetQuickPanelLayout` | `scale *float64, previewExpanded bool, windowScale *float64` | `-` | none | 真实 | 真实 | 真实 | 契约命令；SetQuickPanelLayout sizes the quick panel for the page's content scale, the preview pane and (Linux) the window scale. |
| `set_quick_panel_position` | `SetQuickPanelPosition` | `position QuickPanelPosition` | `-` | command: ValidationError, InternalError | 真实 | 真实 | 真实 | 契约命令；SetQuickPanelPosition saves where the quick panel appears. |
| `set_traffic_light_position` | `SetTrafficLightPosition` | `offsetX *float64, offsetY *float64` | `-` | none | 不支持 | 有意空操作 | 有意空操作 | 契约命令；SetTrafficLightPosition would place the macOS window buttons. |
| `set_tray_language` | `SetTrayLanguage` | `language string` | `-` | none | 真实 | 真实 | 真实 | 契约命令；SetTrayLanguage updates the tray menu labels to the UI language. |
| `set_visual_effects_mode` | `SetVisualEffectsMode` | `mode EffectsMode` | `EffectsSnapshot` | command: ValidationError | 真实 | 真实 | 真实 | 契约命令；SetVisualEffectsMode sets the visual effects preference and tells every window. |
| `set_window_decorations` | `SetWindowDecorations` | `decorations bool` | `-` | none | 有意空操作 | 真实 | 真实 | 适配器：`@/host/window`；SetWindowDecorations applies the page's frame preference (custom controls drawn by the page, or the system frame) to the main window. |
| `show_content_unlock` | `ShowContentUnlock` | - | `-` | none | 真实 | 真实 | 真实 | 契约命令；ShowContentUnlock brings the main window forward so the user can unlock content. |
| `skip_version` | `SkipVersion` | `version string` | `-` | text | 真实 | 真实 | 真实 | 契约命令；SkipVersion remembers that the user does not want the given version on the current channel. |
| `take_pending_navigation` | `TakePendingNavigation` | - | `*string` | none | 真实 | 真实 | 真实 | 契约命令；TakePendingNavigation returns, once, the route a tray or second launch asked the page to open. |
| `type_file_paths_to_previous_app` | `TypeFilePathsToPreviousApp` | `request FilePathInputRequest` | `-` | text | 不支持 | 真实 | 真实 | 契约命令；TypeFilePathsToPreviousApp types file paths, one per line, into the previously focused application. |
| `unlock_content` | `UnlockContent` | `request ContentUnlockRequest` | `-` | unlock | 真实 | 真实 | 真实 | 契约命令；UnlockContent unlocks content with the user's passphrase. |
| `unlock_content_from_keyring` | `UnlockContentFromKeyring` | - | `bool` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；UnlockContentFromKeyring unlocks content with the key kept in the system keychain. |
| `update_autostart` | `UpdateAutostart` | `enabled bool` | `-` | command: InternalError | 真实 | 真实 | 真实 | 契约命令；UpdateAutostart registers or removes the login item and stores the preference. |
| `update_keyboard_shortcuts` | `UpdateKeyboardShortcuts` | `shortcuts map[string]json.RawMessage` | `UpdateKeyboardShortcutsResult` | command: InternalError, Conflict | 真实 | 真实 | 真实 | 契约命令；UpdateKeyboardShortcuts merges a shortcut patch into the settings and re-binds the global shortcut. |

### 事件

| 事件 | Go 常量 | 方向 | 载荷类型 |
| --- | --- | --- | --- |
| `content-lock-changed` | `contentLockChangedEvent` | 宿主 → 页面 | `无载荷` |
| `visual-effects://changed` | `visualEffectsChangedEvent` | 宿主 → 页面 | `EffectsSnapshot` |
| `notification://action` | `notificationActionEvent` | 宿主 → 页面 | `NotificationAction` |
| `update-available` | `updateAvailableEvent` | 宿主 → 页面 | `*UpdateMetadata` |
| `update-download-progress` | `updateProgressEvent` | 宿主 → 页面 | `DownloadEvent` |
| `update-install-progress` | `updateInstallEvent` | 宿主 → 页面 | `DownloadEvent` |
| `settings://sync-changed` | `settingsSyncChangedEvent` | 宿主 → 页面 | `无载荷` |
| `ui://navigate` | `uiNavigateEvent` | 宿主 → 页面 | `string` |
| `quick-panel://prepare-show` | `quickPanelPrepareShow` | 宿主 → 页面 | `无载荷` |
| `app://shutting-down` | `appShuttingDownEvent` | 宿主 → 页面 | `无载荷` |
| `app://daemon-connection-changed` | `daemonConnectionChanged` | 宿主 → 页面 | `无载荷` |
| `desktop-theme://changed` | `desktopThemeChangedEvent` | 宿主 → 页面 | `DesktopThemeSnapshot` |
| `settings://changed` | `settingsChangedEvent` | 页面 → 宿主 | `SettingsChanged` |
| `devices://sync-changed` | `devicesChangedEvent` | 双向 | `string` |

<!-- END GENERATED: host-contract -->

## 与 daemon HTTP / OpenAPI 契约的边界

宿主只负责原生能力与进程生命周期，不复制 daemon 的契约。`get_profile_recovery` 与 `get_daemon_startup_status` 把 daemon 的 JSON 原样透传（Go 侧为 `json.RawMessage`，Wails 生成 `any`）：

- `ProfileRecoveryResponse`、`ShortcutKeyDto` 的类型来源是 OpenAPI 生成的 `apps/gui-go/frontend/src/api/generated/types.gen.ts`，`ipc.ts` 在边界处套用。
- 启动进度 `/startup` 不在 OpenAPI 文档中，其 TypeScript 形态是手工镜像 `apps/gui-go/frontend/src/lib/daemon-startup-types.ts`（来源 `crates/uc-daemon-contract/src/startup.rs`）。该路由进入 OpenAPI 后应删除镜像。
- `update_keyboard_shortcuts` 的值既可以是字符串也可以是字符串数组，Wails 无法表达该联合，生成类型为 `any`，由 `ipc.ts` 按 `ShortcutKeyDto` 收窄。

## 事件审计

前端每个监听的事件都对应一个发射方，或在下面说明原因：

| 事件 | 结论 |
| --- | --- |
| `lifecycle://event` | 唯一的监听方 `useLifecycleStatus` 没有任何使用者，属于死代码，已连同监听一起删除。 |
| `app://shutting-down` | 页面需要在 daemon 停止前主动关闭 WebSocket，否则 daemon 要等心跳超时（约 30 秒）才能退出。宿主现在在停止 daemon 之前（退出与 `restart_daemon`）发射该事件并等待 300 毫秒。 |
| `app://daemon-connection-changed` | `restart_daemon` 会换掉 daemon 进程，新进程有新的连接文件与令牌。宿主现在重新读取连接信息并替换客户端（`HostService.daemon()`），然后发射该事件让页面重连。 |
| `desktop-theme://changed` | Omarchy 主题源在 Go 宿主中未实现（`get_desktop_theme` 恒为不可用），没有可发射的变化，页面监听受可用性限制。事件已按类型声明，并登记为“无发射方（不支持）”，登记理由见 `scripts/architecture/check-host-events.mjs`。 |

## 窗口、日志与 WebView 适配器审计

适配器位于 `apps/gui-go/frontend/src/host/`。下表记录每项在各系统上的真实程度（真实 / 有意空操作 / 不支持）与调用方：

| 适配器接口 | macOS | Windows | Linux | 调用方与说明 |
| --- | --- | --- | --- | --- |
| `window.setDecorations` | 真实 | 真实 | 真实 | `set_window_decorations` 命令，页面按平台决定自绘窗口控制 |
| `window.setTheme` | 有意空操作 | 有意空操作 | 有意空操作 | `lib/window-theme.ts` 忽略其结果；Wails 没有按窗口切换主题的运行时接口，改由页面背景色表达 |
| `window.setBackgroundColor`、`minimize`、`maximize`、`close`、`show`、`setFocus`、`isMaximized` | 真实 | 真实 | 真实 | 直接调用 Wails `Window` |
| `window.startDragging` | 有意空操作 | 有意空操作 | 有意空操作 | 拖动由 `host.css` 把 `data-tauri-drag-region` 映射为 `--wails-draggable: drag` 实现，`useWindowDragging` 的调用是冗余但无害的 |
| `window.onResized` | 真实 | 真实 | 真实 | 订阅 `common:WindowDidResize` |
| `webview.setZoom` | 真实 | 真实 | 真实 | `lib/ui-scale.ts`，调用 Wails `Window.SetZoom` |
| `set_traffic_light_position` 命令 | 不支持 | 有意空操作 | 有意空操作 | Wails 没有交通灯位置接口；调用方 `useMacTrafficLightPosition` 仅在 macOS 运行，位置固定为隐藏内嵌标题栏的默认值 |
| `log.attachConsole` | - | - | - | 调用方被旧宿主的全局标记守卫，而 Go 宿主只设置 `__UC_DESKTOP_HOST__`，该分支从未执行；调用、适配器与别名已一并删除 |

## 迁移后检查清单

新增宿主命令：给 `*HostService` 添加导出方法，写英文文档注释与 `//uc:errors`、`//uc:os` 指令，运行 `bun run gen:host-contract`、`go run ./cmd/hostcontract docs`、`go run ./cmd/hostcontract e2e-table`，并把 E2E 覆盖补进 `apps/gui-go/e2e/`。任何导出方法都会成为命令，因此不要把内部辅助方法导出到 `HostService` 上。
