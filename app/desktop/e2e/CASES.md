# Desktop E2E cases

Generated from `cases/*.txtar`. Run `make desktop-e2e`; select with `CASE=<id>`.

| Case | Mode | Purpose |
|---|---|---|
| [boardui-composition](#boardui-composition) | view | Retain the existing complete input, control, navigation, layout and overlay interaction coverage in one BoardUI design system and both color schemes. |
| [boardui-menus](#boardui-menus) | view | Retain filter/action menu cancellation, visible choices and selected-value dispatch using real in-window BoardUI popups. |
| [boardui-palette](#boardui-palette) | view | Verify brand-only and complete light/dark palettes drive real controls without source-blue primary states or lost caller state. |
| [boardui-production-migration](#boardui-production-migration) | view | Verify complete production history/settings/return and quick-panel compositions use source-bound BoardUI resources with no legacy UI Kit dependency or appearance branch. |
| [boardui-settings](#boardui-settings) | view | Exercise the independent BoardUI Go package through a complete settings form with real input, selection and dialog scopes. |
| [boardui-states](#boardui-states) | view | Exercise fixed-source BoardUI component variants and narrow reduced-motion behavior through the public native API. |
| [boardui-surfaces](#boardui-surfaces) | view | Retain full-view hover feedback, pointer-exit reset, visible actions and send-popup cancellation in the unified BoardUI design system. |
| [boardui-theme](#boardui-theme) | view | Resolve custom light/dark tokens through public controls, application composition and native mygo defaults. |
| [builtin-plugin-policy](#builtin-plugin-policy) | native | Save Tailcat content policies and automatic thresholds from the real Plugins page. |
| [builtin-plugin-processors](#builtin-plugin-processors) | native | Manage classification and text tools from Plugins and verify actual behavior after Desktop restart. |
| [data-binding](#data-binding) | process | History, tags, restore, pairing and reconnect through real services. |
| [desktop-lightweight-close](#desktop-lightweight-close) | process | Verify the updated lightweight user intent through real role and service processes. |
| [desktop-lightweight-invalid-config-close](#desktop-lightweight-invalid-config-close) | process | Verify that a bad TOML edit cannot strand tray or panel after a lightweight GUI closes. |
| [desktop-native-processes](#desktop-native-processes) | native | Verify independent GUI, quick-panel, tray and service processes through native windows with an isolated directory clipboard. |
| [desktop-processes](#desktop-processes) | process | Normal runtime retains tray and service after GUI close and independently recreates GUI and quick-panel processes. |
| [event-wakeup](#event-wakeup) | process | Exercise actual service events, content completions and recovery through stopped production renderers. |
| [external-sync-workflow](#external-sync-workflow) | native | Configure a runtime instance and pair through an external Go backend with Tailcat disabled. |
| [first-install-setup](#first-install-setup) | process | Verify setup persistence and migration with real Engine, SQLite and a service process. |
| [gui-approval-reconnect](#gui-approval-reconnect) | view | Legacy GUI fingerprint approval and recovery after service disconnection. |
| [gui-empty-layout](#gui-empty-layout) | view | Legacy GUI empty-history layout and narrow-window bounds. |
| [gui-navigation](#gui-navigation) | view | Legacy GUI navigation, pairing input and destructive-confirmation barriers. |
| [history-favorites](#history-favorites) | native | Verify persistent favorites, chronological history and the hidden reserved tag using production history. |
| [history-search-copy](#history-search-copy) | native | Connect visible user actions to production history and clipboard application. |
| [i18n](#i18n) | view | Exercise five language catalogues through the shared desktop composition. |
| [idle-event-wakeup](#idle-event-wakeup) | native | Verify native service event and content completion delivery. |
| [image-previews](#image-previews) | process | Exercise thumbnail ownership, original selection, original bytes, EXIF orientation, shown-pixel lifecycle and serial image reads through real service content reads. |
| [incremental-browsing](#incremental-browsing) | process | Multi-page production browsing, idle request timeline, coalescing, selection and cancellation. |
| [interaction-motion](#interaction-motion) | view | Fixed-source 0.98 button press/release, cancellation, selection reversal, in-window popups and Reduce Motion. |
| [pairing-inline-approval](#pairing-inline-approval) | native | Approve an incoming device from the invitation page of the real Desktop. |
| [production-boundary](#production-boundary) | process | Build independent production and demo commands and verify their dependency and embedded catalogue boundaries. |
| [quick-panel-contract](#quick-panel-contract) | process | Target session, clipboard application, cancellation and paste-request ordering with OS fixtures. |
| [quick-panel-screenshots](#quick-panel-screenshots) | view | Quick-panel history, action menu and image-grid compositions in both themes. |
| [quick-panel](#quick-panel) | view | Quick-panel keyboard, search, actions, previews, scrolling and simulated effects. |
| [search-badge-delete](#search-badge-delete) | view | Whole-condition deletion from composite search badges. |
| [search-contract](#search-contract) | process | Literal matching, paging, counts, candidates, repair, cancellation, viewport detail prefetch and persistent favorites. |
| [search-disclosure-reduced-motion](#search-disclosure-reduced-motion) | view | Toolbar search disclosure with Reduce Motion enabled. |
| [search-disclosure](#search-disclosure) | view | Animated toolbar search expansion, collapse, filtering and keyboard dismissal. |
| [send-device-menu](#send-device-menu) | view | Send-menu device projection, disabled targets and selected-device dispatch. |
| [settings-appearance](#settings-appearance) | process | Verify appearance defaults, saves, rejection, external edits and service restart without a renderer. |
| [settings-backup](#settings-backup) | process | Verify portable allowlisting, strict preflight, preview confirmation, revision conflicts and restart persistence. |
| [settings-diagnostics](#settings-diagnostics) | process | Verify persistent logging, bounded overrides, telemetry controls and safe diagnostic export through production processes. |
| [settings-shortcuts](#settings-shortcuts) | process | Exercise the production Desktop binding against a private service with controlled permission-host callbacks. |
| [settings-storage](#settings-storage) | process | Read the real service cache ledger through the production settings model without scanning or deleting user files. |
| [settings-visual-evidence](#settings-visual-evidence) | native | Capture the actual settings composition for diagnostics, storage, appearance, permission guidance and portable backup. |
| [setup-later-migration](#setup-later-migration) | native | Resume a later migration after cancelling and reopening the real Desktop. |
| [setup-onboarding](#setup-onboarding) | native | Record the real welcome, migration confirmation, passphrase fallback, import, verification and history entry. |
| [setup-pages](#setup-pages) | view | Exercise the production BoardUI setup renderer in light and dark. |
| [setup-partial](#setup-partial) | native | Record an honest partial result and entry into usable migrated history. |
| [sheet-exit-stability](#sheet-exit-stability) | view | Sheet exit frames and input stability after dismissal. |
| [sheet-motion](#sheet-motion) | view | Real Sheet entry and dismissal by Done, Escape and backdrop, live resize/reversal, and inline tag confirmation or cancellation, including Reduce Motion. |
| [sidebar-collapse](#sidebar-collapse) | view | Sidebar closing tail, reopening and interrupted-transition reversal. |
| [sidebar-navigation](#sidebar-navigation) | view | Distinguish hover from selection and keep scrollbar/footer space in the real history sidebar. |
| [source-application](#source-application) | view | Show the captured application's cached icon and name immediately after device and time. |
| [startup-lifecycle](#startup-lifecycle) | process | Ownership, background intent, lightweight launch and process cleanup. |
| [storage-cleanup-settings](#storage-cleanup-settings) | native | Keep cleanup rules inactive when cleanup is off while preserving the independent cache quota. |
| [system-clipboard-formats](#system-clipboard-formats) | native | Prove native MIME capture and application without shared-clipboard propagation. |
| [system-clipboard-receive](#system-clipboard-receive) | native | Verify incoming production synchronization reaches the native clipboard without sender echo. |
| [system-clipboard-target](#system-clipboard-target) | native | Prove actual cross-application insertion independently from clipboard application. |
| [system-clipboard](#system-clipboard) | native | Establish a real system clipboard baseline, fresh capture and exact application without echo. |
| [system-linux-ui-dark](#system-linux-ui-dark) | native | Inspect the real Linux skin, Chinese glyphs, search focus, menu and 600-point layout in dark appearance. |
| [system-linux-ui-light](#system-linux-ui-light) | native | Inspect the real Linux skin, Chinese glyphs, search focus, menu and 600-point layout in light appearance. |
| [system-managed-cleanup](#system-managed-cleanup) | native | Verify managed received files and native clipboard leases under unified cleanup. |
| [system-offline-backlog](#system-offline-backlog) | native | Separate persistent offline history from fresh native clipboard delivery. |
| [system-second-launch](#system-second-launch) | native | Isolate second-launch activation from the separately tested capture without GUI requirement. |
| [system-wayland-panel](#system-wayland-panel) | native | Verify understandable automatic-paste refusal, preserved Copy and physical Escape dismissal on Wayland. |
| [system-window-lifecycle](#system-window-lifecycle) | native | Verify that closing the native main window preserves tray capture and second launch creates a new GUI process. |
| [tag-editor](#tag-editor) | view | V1-style inline tag creation, five swatches, removable tinted chips and shared Sheet colors. |
| [tag-fixed-columns](#tag-fixed-columns) | view | Tag-table fixed-column geometry in both BoardUI themes. |
| [tag-manager-platforms](#tag-manager-platforms) | view | Tag management and separate entry assignment in the single BoardUI design system. |
| [tag-manager](#tag-manager) | view | Tag management, assignment, rename, merge, deletion and protected built-in tags. |
| [token-search](#token-search) | view | Composite search prefixes, counts, completion, literal input, caret handling and Escape. |
| [toolbar-alignment](#toolbar-alignment) | view | Toolbar containment and menu alignment across supported widths, themes, scales and layouts, including light-theme control pixel contrast. |
| [toolbar-bounds-dark-1100](#toolbar-bounds-dark-1100) | native | Keep unified production history controls within the toolbar at 1100x720. |
| [toolbar-bounds-dark-600](#toolbar-bounds-dark-600) | native | Keep unified production history controls within the toolbar at 600x480. |
| [toolbar-bounds-light-1100](#toolbar-bounds-light-1100) | native | Keep unified production history controls within the toolbar at 1100x720. |
| [toolbar-bounds-light-600](#toolbar-bounds-light-600) | native | Keep unified production history controls within the toolbar at 600x480. |
| [tray-icon-settings](#tray-icon-settings) | view | Match v1 glyph/cat defaults, settings selection and immediate image delivery. |
| [unified-scrollbars](#unified-scrollbars) | view | Scrollbar visibility during hover, scroll, idle and nested preview interaction. |
| [wetype-pairing](#wetype-pairing) | view | Join the first shared space with a six-digit code using the installed default plugin instance. |
| [window-chrome-native-dark](#window-chrome-native-dark) | native | Verify the shared frame switch and pending state against GUI lifetime rather than daemon lifetime. |
| [window-chrome-native-input](#window-chrome-native-input) | native | Verify physical input on the owned main window without clipboard access. |
| [window-chrome-native](#window-chrome-native) | native | Verify the shared frame switch and pending state against GUI lifetime rather than daemon lifetime. |
| [window-chrome-setup](#window-chrome-setup) | native | Keep Windows caption actions available before the ordinary application toolbar exists. |
| [window-chrome-startup-error](#window-chrome-startup-error) | native | Keep Windows custom controls available when real service startup fails. |
| [window-chrome](#window-chrome) | view | Select native title bars on all platforms and put Windows custom controls in the application toolbar. |

## boardui-composition

BoardUI application composition interactions

Steps: Execute all migrated widget-view scenarios through real mygo event, layout, focus and rendering; retain the final exercised PNG for every view, including failures.

Expected: Every retained scenario and applicable light/dark subcase passes without old OS-skin selection or missing evidence.

Forbidden: New isolated unit tests, deleted interaction assertions, hidden skips, legacy token maps, or software-view screenshots reported as native OS proof.

```text
acceptance desktopkit boardui-composition
```

## boardui-menus

BoardUI production popup dispatch

Steps: Open the unified filter popover and action menus, inspect accessible choices, cancel without mutation, reopen and choose each filter dimension or command in both themes.

Expected: Chip choices apply immediately and survive popover dismissal; action menus dispatch and close; cancellation changes no value; legacy system menus stay absent.

Forbidden: Direct command invocation behind the view, lost cancellation or persistence checks, dropped options, or software views labeled native OS acceptance.

```text
acceptance desktopui boardui-menus
```

## boardui-palette

Twelve-color palette authoring and derived mygo states

Steps: Define one-color and twelve-color palettes, edit an input, toggle a switch, activate buttons, switch themes, inspect rendered backgrounds and native colors, and apply exact token overrides.

Expected: Empty palettes preserve defaults, light brands receive readable foregrounds, all primary states derive from the brand, exact overrides remain isolated, and PNG/accessibility checkpoints plus resolved palette JSON are exported.

Forbidden: Native-window or contrast-certification claims, global palette mutation, resetting caller data, or missing failure artifacts.

```text
acceptance boardui boardui-palette
```

## boardui-production-migration

Production Desktop uses one BoardUI design system

Steps: Inspect actual production source; ignore hostile old platform-skin overrides; operate history to settings, edit synthetic Unicode data, return to history and inspect the quick panel in both themes and scales.

Expected: The old Kit directory and imports are absent, source-derived BoardUI font/theme resources apply consistently, navigation and caller state survive and all exercised views produce PNG and structured receipts.

Forbidden: Renaming legacy skins as BoardUI, product profile or clipboard access, skipped legacy dependencies, static image interaction, or software views labeled as native platform acceptance.

```text
acceptance desktopui boardui-production-migration
```

## boardui-settings

BoardUI native settings composition

Steps: Open the settings dialog, edit synthetic English/CJK/emoji text, validate empty input, choose Role by keyboard, dismiss nested overlays, traverse focus and save retained caller state across themes and scales.

Expected: The complete operation chain passes with accessible field states, bounded overlays, restored focus and exercised software-rendered PNG checkpoints.

Forbidden: Product UI replacement, system clipboard access, skipped states, static-image interaction substitutes, or software-view evidence labeled as native-window acceptance.

```text
acceptance boardui boardui-settings
```

## boardui-states

BoardUI component states and narrow layout

Steps: Drive the registered state matrix, inspect accessible values and geometry, and capture exercised states including the narrow-window select.

Expected: Required variant and state assertions pass and each exercised view emits screenshots with explicit source and renderer scope.

Forbidden: Unsupported states silently counted as delivered, altered reference source, lost focus, unbounded popup geometry, or software-view images reported as OS accessibility proof.

```text
acceptance boardui boardui-states
```

## boardui-surfaces

BoardUI production surface interaction

Steps: Exercise view-switch, search and secondary-action surfaces in both themes at 2x, sample real surface pixels, open the in-window send popup and cancel it.

Expected: Hover feedback resets, action geometry remains visible, actual popup items appear and cancellation performs no action.

Forbidden: Requiring removed OS-glass rims, legacy system-menu delegation, dropped functional assertions, or software raster checks labeled native GPU evidence.

```text
acceptance desktopui boardui-surfaces
```

## boardui-theme

Code-defined shared mygo color theme

Steps: Edit a real input, activate a primary button, switch light/dark/light, inspect native colors and gradient resolution, and restore the default theme.

Expected: Overrides remain immutable, unknown names fail, native colors and gradients share tokens, caller state survives changes, and PNG/accessibility checkpoints plus resolved token JSON are retained.

Forbidden: Native-window or OS integration acceptance claims, global theme mutation, or resetting caller state.

```text
acceptance boardui boardui-theme
```

## builtin-plugin-policy

Built-in Tailcat policy through Desktop settings

Steps: Toggle each content permission, validate Engine decisions, check directory grouping and total-byte threshold, then reopen Desktop.

Expected: Saved policies persist; Engine decisions change immediately; manual sends bypass only the automatic threshold.

Forbidden: Fixture settings, unsupported Tailcat disablement, altered global permissions, or false runtime success.

```text
start
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 插件
ui-click listItem 插件
ui-wait button 'Tailcat 同步'
ui-enabled button 'Tailcat 同步'
ui-click button 'Tailcat 同步'
ui-wait switch '同步文本'
plugin-back-in-toolbar
screenshot tailcat-defaults
ui-enabled switch '同步文本'
ui-click switch '同步文本'
config-equals plugins.instances.builtin.tailcat.sync.text false
ui-click listItem 同步
ui-enabled switch '启用同步'
ui-click switch 启用同步
config-equals sync.enabled true
ui-click listItem 插件
plugin-probe text policy_filtered
ui-enabled switch '同步文本'
ui-click switch '同步文本'
config-equals plugins.instances.builtin.tailcat.sync.text true
plugin-probe text no_targets
ui-enabled switch '同步图片'
ui-click switch '同步图片'
config-equals plugins.instances.builtin.tailcat.sync.images false
plugin-probe image policy_filtered
ui-enabled switch '同步图片'
ui-click switch '同步图片'
config-equals plugins.instances.builtin.tailcat.sync.images true
plugin-probe image no_targets
ui-enabled switch '同步文件与文件夹'
ui-click switch '同步文件与文件夹'
config-equals plugins.instances.builtin.tailcat.sync.files false
plugin-probe file policy_filtered
plugin-probe directory sync_disabled
ui-enabled switch '同步文件与文件夹'
ui-click switch '同步文件与文件夹'
config-equals plugins.instances.builtin.tailcat.sync.files true
plugin-probe directory no_targets
ui-enabled textField '自动文件传输阈值（MB）'
ui-click textField '自动文件传输阈值（MB）'
ui-set textField '自动文件传输阈值（MB）' '-1'
ui-key Enter
ui-wait text '请输入 0 到 4096 之间的整数。'
ui-invalid textField '自动文件传输阈值（MB）'
config-equals plugins.instances.builtin.tailcat.sync.auto_file_max_bytes 0
ui-enabled textField '自动文件传输阈值（MB）'
ui-click textField '自动文件传输阈值（MB）'
ui-set textField '自动文件传输阈值（MB）' '1'
ui-key Enter
config-equals plugins.instances.builtin.tailcat.sync.auto_file_max_bytes 1048576
connect-policy-peer
plugin-auto-probe file auto_file_too_large
plugin-probe large-file queued
ui-click listItem 同步
ui-enabled switch 自动同步目录
ui-click switch 自动同步目录
config-equals sync.directories.automatic true
ui-click listItem 插件
plugin-auto-probe directory auto_file_too_large
plugin-probe large-directory queued
ui-enabled textField '自动文件传输阈值（MB）'
ui-click textField '自动文件传输阈值（MB）'
ui-set textField '自动文件传输阈值（MB）' '0'
ui-key Enter
config-equals plugins.instances.builtin.tailcat.sync.auto_file_max_bytes 0
plugin-auto-probe directory queued
ui-enabled textField '自动文件传输阈值（MB）'
ui-click textField '自动文件传输阈值（MB）'
ui-set textField '自动文件传输阈值（MB）' '1'
ui-key Enter
config-equals plugins.instances.builtin.tailcat.sync.auto_file_max_bytes 1048576
screenshot tailcat-saved
stop-policy-peer
restart-desktop
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 插件
ui-click listItem 插件
ui-enabled button 'Tailcat 同步'
ui-click button 'Tailcat 同步'
config-equals plugins.instances.builtin.tailcat.sync.auto_file_max_bytes 1048576
plugin-probe text no_targets
screenshot tailcat-reopened
```

## builtin-plugin-processors

Built-in processors saved and applied from Desktop

Steps: Disable each plugin, check the failure-policy dropdown shows inheritance, check pending restart, restart, verify tags and Actions, then enable both again. The data-binding acceptance chooses dropdown values through the system menu; this driver cannot.

Expected: Classification alone reports restart; old runtime remains active until restart; saved changes survive restart and affect new history and Actions.

Forbidden: Premature runtime claims, lost settings, missing restart notice, fixture-only acceptance or old service residue.

```text
start
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 插件
ui-click listItem 插件
ui-enabled button '内容分类'
ui-click button 内容分类
plugin-back-in-toolbar
screenshot classification-toolbar
ui-enabled switch '启用内容分类'
ui-click switch 启用内容分类
config-equals plugins.instances.builtin.classify.enabled false
restart-required plugins
ui-wait text '插件设置已保存。请退出并重新启动桌面端以生效。'
ui-enabled switch '启用内容分类'
ui-click switch 启用内容分类
config-equals plugins.instances.builtin.classify.enabled true
restart-clear
ui-enabled switch '启用内容分类'
ui-click switch 启用内容分类
config-equals plugins.instances.builtin.classify.enabled false
ui-value popUpButton 处理失败时 '跟随全局（继续处理）'
config-equals plugins.instances.builtin.classify.on_error null
ui-enabled button '返回插件列表'
ui-click button 返回插件列表
ui-absent button 返回插件列表
ui-enabled button '文本工具'
ui-click button 文本工具
ui-enabled switch '启用文本工具'
ui-click switch 启用文本工具
config-equals plugins.instances.builtin.text.enabled false
ui-value popUpButton 处理失败时 '跟随全局（继续处理）'
config-equals plugins.instances.builtin.text.on_error null
processor-probe enabled
screenshot processors-pending
restart-desktop
processor-probe disabled
restart-clear
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 插件
ui-click listItem 插件
ui-enabled button '内容分类'
ui-click button 内容分类
config-equals plugins.instances.builtin.classify.enabled false
ui-enabled switch '启用内容分类'
ui-click switch 启用内容分类
config-equals plugins.instances.builtin.classify.enabled true
ui-enabled button '返回插件列表'
ui-click button 返回插件列表
ui-absent button 返回插件列表
ui-enabled button '文本工具'
ui-click button 文本工具
ui-enabled switch '启用文本工具'
ui-click switch 启用文本工具
config-equals plugins.instances.builtin.text.enabled true
screenshot processors-enable-pending
restart-desktop
processor-probe enabled
restart-clear
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 插件
ui-click listItem 插件
screenshot processors-active
ui-enabled button 文本工具
ui-click button 文本工具
ui-value popUpButton 处理失败时 '跟随全局（继续处理）'
config-equals plugins.instances.builtin.text.on_error null
restart-clear
config-equals telemetry.enabled false
config-equals sync.enabled false
screenshot processors-policy-restored
```

## data-binding

Desktop data binding

Steps: Execute the registered production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance desktopui data-binding
```

## desktop-lightweight-close

Lightweight manual launch and GUI-close lifetime

Steps: Keep an independent service running; verify manual close, owned-service detachment, background intent, concurrent reopening and live settings; then run four cold-service background/manual launch races, including observation-triggered manual launches.

Expected: Manual launches show GUI; lightweight close stops all UI while retaining the same unicd PID and generation; an owned service detaches to standalone; explicit lightweight background runs only unicd; normal background retains tray and a hidden panel without GUI; concurrent reopening leaves one visible GUI.

Forbidden: Treating saved start_in_background as manual-launch intent, leaving a hidden GUI/tray/panel, stopping the independent service, replacing its generation, or reporting process fixtures as native focus evidence. Failure inventory specified before production changes: Manual launch remains service-only or obeys saved background preference; lightweight close retains UI or stops/replaces unicd; an owned daemon exits on owner EOF; reopening reuses an exited GUI; background mode creates unwanted UI; close acknowledgement races with reopening and loses the latest manual intent; live mode/background saves report an obsolete restart requirement or GUI close consumes stale mode; cold background AcquireService releases its launch lock before Detach and a concurrent manual tray rejects that transient desktop-owned service. Background launch note: The --background argument represents explicit login/background intent. Manual launch always has no arguments.

```text
acceptance startup desktop-lightweight-close
```

## desktop-lightweight-invalid-config-close

Lightweight GUI close preserves the last valid mode after a malformed config edit

Steps: Start a real independent service and lightweight GUI; save valid lightweight mode through the production CLI; open the independent panel; write malformed TOML in the private profile; close GUI; restore the private file during cleanup.

Expected: GUI, tray and panel processes and IPC terminate using the last valid lightweight mode; the exact independent service PID and generation remain.

Forbidden: Returning early on configuration reload error, leaving a tray/panel resident, stopping or replacing unicd, touching a personal profile, or presenting process fixtures as native UI evidence. Failure inventory specified before production changes: The config watcher retains valid lightweight state but the GUI-closed handler returns the reload error before applying that state; GUI exits while tray and panel remain.

```text
acceptance startup desktop-lightweight-invalid-config-close
```

## desktop-native-processes

Independent native Desktop windows and process lifetimes

Steps: Record role identities; request the panel through tray IPC; copy real service history through the native panel; close the GUI process; reopen it through tray IPC and inspect the new process and window.

Expected: Every role has a distinct PID; panel Copy writes exact directory-board bytes; standby has no windows while the PID survives; reopening renders fresh history and satellite; GUI close terminates its process; tray and service remain; tray IPC creates a new native GUI process with retained history.

Forbidden: System clipboard access, physical input, native tray-click claims, global shortcut claims, hidden resident GUI, shared role PIDs, missing screenshots or missing continuous sampled-window recording.

```text
start
submit native-role-content 'Desktop independent native process copy'
ui-wait listItem 'Desktop independent native process copy'
role-checkpoint started
screenshot main-started
role-panel-hidden
role-open-panel
ui-wait button '历史：Desktop independent native process copy'
ui-set textField '搜索快捷面板' 'independent native'
ui-wait button '历史：Desktop independent native process copy'
role-panel-preview
screenshot independent-panel
role-panel-copy
board-equals 'Desktop independent native process copy'
role-panel-hidden
submit native-role-standby 'New independent native entry from standby'
role-open-panel
ui-wait button '历史：Desktop independent native process copy'
ui-wait button '历史：New independent native entry from standby'
role-panel-query 'independent native'
role-panel-preview
screenshot recreated-panel
role-panel-copy
board-equals 'Desktop independent native process copy'
role-select-gui
screenshot panel-copy-applied
role-close-gui
role-reopen-gui
ui-wait listItem 'Desktop independent native process copy'
screenshot recreated-gui
```

## desktop-processes

Independent Desktop role processes

Steps: Start normal mode with explicit background intent, verify no initial GUI and a ready hidden panel, then open and close each UI role and verify cleanup. Reject a GUI quit, retry the GUI-only restart, and request a full restart after saving a plugin change.

Expected: Tray, GUI, quick panel and unicd have independent process identities; tray activation works with unicd already running. GUI-only restart returns the quit failure and preserves the tray, quick panel and unicd on retry. Full restart replaces the owned unicd and clears the pending configuration.

Forbidden: Resident hidden GUI, shared role PID, duplicate owners, foreign-profile activation, lost state, leaked processes, or native acceptance claims.

```text
acceptance startup desktop-processes
```

## event-wakeup

Event-driven history and quick-panel rendering

Steps: Start isolated unicd, render both production bindings, advance idle UI time, submit real history and await independent deadline recovery.

Expected: No idle heartbeat builds; async events and content appear without manual frames; recovery still runs.

Forbidden: Manual polling to deliver completions, future 200 ms frame requests, native OS claims or missing receipt.

```text
acceptance desktopui event-wakeup
```

## external-sync-workflow

External Go backend instance through the production Desktop

Steps: Seed a digest-verified loopback fixture installation, create and enable an instance through Plugins, restart, approve cross-instance pairing in the invitation, and verify sync policy and exact bidirectional content.

Expected: Implementation and instance identities stay separate; the visible instance policy affects real transfers; both sides explicitly approve; per-backend results remain inspectable.

Forbidden: Tailcat fallback, shared identity stores, personal clipboard use, production transport claims for the loopback fixture, or approval inferred from device trust.

```text
start-external
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 插件
ui-click listItem 插件
ui-wait button e2e.tcp
ui-click button e2e.tcp
ui-wait textField '实例 ID'
ui-set textField '实例 ID' e2e.desktop
ui-enabled button 创建实例
ui-click button 创建实例
config-equals plugins.instances.e2e.desktop.implementation '"e2e.tcp"'
ui-wait button 返回插件列表
ui-click button 返回插件列表
ui-wait button e2e.desktop
ui-click button e2e.desktop
ui-wait switch 启用
ui-click switch 启用
config-equals plugins.instances.e2e.desktop.enabled null
ui-wait switch 同步文本
ui-click switch 同步文本
config-equals plugins.instances.e2e.desktop.sync.text false
screenshot instance-configured
restart-desktop
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 同步
ui-click listItem 同步
ui-wait switch 启用同步
ui-click switch 启用同步
config-equals sync.enabled true
external-policy-check
ui-click listItem 插件
ui-wait button e2e.desktop
ui-click button e2e.desktop
ui-wait switch 同步文本
ui-click switch 同步文本
config-equals plugins.instances.e2e.desktop.sync.text true
ui-click listItem 返回历史
ui-wait listItem 管理设备…
ui-click listItem 管理设备…
ui-wait button 邀请设备
ui-click button 邀请设备
ui-wait button '跨网或命令行配对？改用完整票据 →'
ui-click button '跨网或命令行配对？改用完整票据 →'
ui-wait button 复制票据
ui-enabled button 复制票据
screenshot external-invitation
start-pairing-peer
peer-pair-desktop
ui-wait text 核对并批准
screenshot external-approval
ui-click button 接受
assert-paired
ui-absent dialog 添加设备
ui-wait button 返回历史
ui-click button 返回历史
external-roundtrip
ui-wait listItem external-desktop-to-peer
ui-click listItem external-desktop-to-peer
ui-wait text '~e2e.desktop.*'
screenshot external-result
stop-policy-peer
```

## first-install-setup

First-install setup and offline v1 import

Steps: Discover synthetic sources; skip and reopen; import with wrong and correct credentials; cancel and resume; repair and retry; start a real service and read imported history and shortcut preferences; reject concurrent migration; stop the owned service, reopen a pending later migration, cancel and resume, then restart service.

Expected: Established profiles bypass setup; unfinished first and later migrations resume; imported records survive cancellation without duplication; partial counts remain distinct; source data remains unchanged; service exposes stable imported history identities after restart; source shortcut settings remain unchanged and a conflicting new default stays unbound.

Forbidden: Native GUI launch, personal profile or credential access, sync, clipboard capture, duplicate import, or automatic termination of an unrelated service.

```text
acceptance startup first-install-setup
```

## gui-approval-reconnect

Gui approval reconnect

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Explicit approval information stays visible and the view recovers after reconnection.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance gui gui-approval-reconnect
```

## gui-empty-layout

Gui empty layout

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Empty state fills the body and focused controls remain visible in both BoardUI themes.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance gui gui-empty-layout
```

## gui-navigation

Gui navigation

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: BoardUI and both themes retain navigation, normalized pairing input and safe dismissal.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance gui gui-navigation
```

## history-favorites

Save and filter favorites in the native desktop

Steps: Submit two text entries. Favorite the older entry and select Favorites. Restart and inspect Favorites and the tag manager. Remove the favorite.

Expected: Both entries appear in All Items with the original newer entry first. Favorites shows only the older entry before and after restart. The tag manager omits the reserved tag. Removing the favorite empties Favorites and leaves both entries in All Items.

Forbidden: Demo rows, system clipboard writes, missing screenshots or missing continuous recording.

```text
start
submit favorite '收藏验证：保留这条记录'
submit ordinary '普通记录：仍在全部项目中'
ui-wait listItem '收藏验证：保留这条记录'
ui-wait listItem '普通记录：仍在全部项目中'
ui-click listItem '收藏验证：保留这条记录'
ui-enabled button '切换收藏'
ui-click button '切换收藏'
ui-click listItem '已收藏'
ui-wait listItem '收藏验证：保留这条记录'
ui-absent listItem '普通记录：仍在全部项目中'
screenshot favorites-filter
ui-click listItem '全部项目'
ui-wait listItem '普通记录：仍在全部项目中'
ui-wait listItem '收藏验证：保留这条记录'
screenshot all-items-after-favoriting
restart-desktop
ui-wait listItem '已收藏'
ui-click listItem '已收藏'
ui-wait listItem '收藏验证：保留这条记录'
ui-absent listItem '普通记录：仍在全部项目中'
screenshot favorites-after-restart
ui-click button '管理标签'
ui-wait text '~系统类型.*'
ui-absent text '#favorite'
screenshot tag-manager
ui-click button '完成'
ui-enabled button '切换收藏'
ui-click button '切换收藏'
ui-wait heading '还没有收藏的内容'
screenshot favorite-removed
ui-click listItem '全部项目'
ui-wait listItem '收藏验证：保留这条记录'
ui-wait listItem '普通记录：仍在全部项目中'
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

Steps: Select each language through settings, navigate to history, filter files, verify switches have one visible row caption and retain accessible names and mouse/keyboard behavior, and render quick-panel actions in BoardUI and both themes.

Expected: Navigation and actions use the selected language; content and filter identities remain unchanged; catalogues and format arguments are complete.

Forbidden: Missing translations or translated user content.

```text
acceptance desktopui i18n
```

## idle-event-wakeup

Idle desktop wakes for asynchronous service changes

Steps: Start an isolated desktop, await one history item and idle UI, submit another item, then await its display and idle UI.

Expected: Current native work settles and service changes appear without input or manual refresh; event-wakeup checks future heartbeat timers.

Forbidden: Unsettled current frames, lost completions, fixture history, system clipboard writes or missing native evidence.

```text
start
submit initial 'Memory acceptance initial text'
ui-wait listItem 'Memory acceptance initial text'
ui-idle
screenshot idle
submit changed 'Memory acceptance asynchronous change'
ui-wait listItem 'Memory acceptance asynchronous change'
ui-idle
screenshot changed
```

## image-previews

Bounded history image previews

Steps: Submit synthetic PNG and JPEG-file history, prefetch both production views, switch selection and reread stored bytes, repaint the selected preview in software and count concurrent image-body readers.

Expected: Neighbor thumbnails are bounded; selected images keep original dimensions; old selections downgrade; original bytes and EXIF orientation remain correct; software repaint preserves the selected preview and its decoded CPU pixels; image-body consumption and response ownership are serial; selected work wins queued neighbors; cancellation and shutdown return slots; thumbnail input streams through bounded reads; malformed streams are rejected.

Forbidden: Full rasters for every neighbor, changed clipboard payloads, stale selection tiers, skipped assertions or missing receipt.

```text
acceptance desktopui image-previews
```

## incremental-browsing

Incremental history browsing

Steps: Start isolated unicd, populate synthetic history, and exercise the production Desktop binding.

Expected: Idle history stays unchanged; paging and history events preserve browsing context.

Forbidden: Native UI claims, skipped assertions, missing source/binary hashes or receipt.

```text
acceptance desktopui incremental-browsing
```

## interaction-motion

Interaction motion

Steps: Execute the registered scenario and all its BoardUI interaction subcases.

Expected: Geometry and rendered selection feedback follow the complete interaction sequence.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui interaction-motion
```

## pairing-inline-approval

Pairing approval without leaving the invitation

Steps: Open the device sheet, show a ticket invitation, let a second production service join and approve, then approve in the same task panel.

Expected: The fingerprint approval replaces the invitation inside the task; accepting it closes the task and lists the new device.

Forbidden: Returning to the device list to approve, a pending request left behind, or fixture-only device data.

```text
start
start-pairing-peer
ui-wait listItem 管理设备…
ui-click listItem 管理设备…
ui-wait button 邀请设备
ui-click button 邀请设备
ui-wait button '跨网或命令行配对？改用完整票据 →'
ui-click button '跨网或命令行配对？改用完整票据 →'
ui-wait button 复制票据
ui-enabled button 复制票据
ui-wait heading 添加设备
screenshot invitation-shown
peer-pair-desktop
ui-wait text '核对并批准'
screenshot approval-inside-task
ui-click button 接受
assert-paired
ui-absent text '核对并批准'
ui-absent dialog 添加设备
ui-wait text 本机
screenshot paired-device-listed
observe 2
ui-wait button 返回历史
ui-click button 返回历史
ui-absent dialog 添加设备
stop-policy-peer
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

Steps: Delay the initial clipboard baseline; wait for real adapter command readiness; execute the registered production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance quickpanel quick-panel-contract
```

## quick-panel-screenshots

Quick panel screenshots

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Six composite PNGs retain source identity, image hashes and view rectangles.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui quick-panel-screenshots
```

## quick-panel

Quick panel

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: All original panel scenarios retain their selected entries and expected simulated effects.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui quick-panel
```

## search-badge-delete

Search badge delete

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Backspace and remove actions reset only the selected condition dimension.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui search-badge-delete
```

## search-contract

History search contract

Steps: Execute the registered production-process suite and validate its receipt.

Expected: Favorites persist across a service restart, combine with search filters and remain hidden from tag catalogues. The suite emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance desktopui search-contract
```

## search-disclosure-reduced-motion

Search disclosure reduced motion

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Disclosure reaches its expected geometry without spatial transition.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui search-disclosure-reduced-motion
```

## search-disclosure

Search disclosure

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Search width changes without moving the command capsule or losing input behavior.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui search-disclosure
```

## send-device-menu

Send device menu

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Only valid device choices produce the expected send command.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui send-device-menu
```

## settings-appearance

Persisted Desktop appearance through the production service

Steps: Start a private production service; inspect the default mode; save dark/light/system; reject an invalid mode and a stale revision; reload a valid external edit; restart the owned service and read the saved value.

Expected: Color mode defaults to system, valid changes persist, invalid or stale writes retain the last valid mode, unrelated sync and telemetry settings survive, and service restart preserves the saved appearance.

Forbidden: Native GUI, browser, software renderer, system clipboard, personal profiles, sync or telemetry upload.

```text
acceptance desktopui settings-appearance
```

## settings-backup

Portable settings export and confirmed import

Steps: Start production unicd, drive Desktop backup actions, reject invalid inputs, confirm a portable import and restart.

Expected: Exports exclude local secrets/paths, preview never writes, confirmation preserves excluded local state and refuses stale revisions.

Forbidden: Secret export, implicit import, invalid document mutation, lost external edits, or claims of native dialog acceptance.

```text
acceptance desktopui settings-backup
```

## settings-diagnostics

Diagnostic settings and safe local export

Steps: Drive the production Desktop binding against unicd, restart, and inspect an archive made by the production CLI.

Expected: Saved and effective values agree, overrides reset, telemetry retains signal preferences, and export preserves unrelated files.

Forbidden: A skipped scenario, stale status labeled current, silent conflict overwrite, secret archive membership, or claims of native dialog acceptance.

```text
acceptance desktopui settings-diagnostics
```

## settings-shortcuts

Shortcut persistence, sequence dispatch, and explicit System Settings guidance

Steps: Save a two-step shortcut and a disabled binding; reopen and dispatch the saved sequence; reject a conflicting prefix; read denied/granted/unsupported/error states; explicitly request settings guidance without granting permission; overlap an outstanding read with opening settings; replace callbacks and cancel a late completion.

Expected: Bindings survive reopening and rejected conflicts preserve the revision; reads never request permission or open settings; opening settings never grants permission; operations do not block each other; stale or cancelled results do not replace current state.

Forbidden: Native permission calls, GUI, renderer, browser, system settings launch, system clipboard, OS shortcut registration.

```text
acceptance desktopui settings-shortcuts
```

## settings-storage

Storage inventory and directory action

Steps: Start an isolated service, refresh inventory, preserve unrelated files, restart with a custom destination, and exercise directory callbacks and failed refresh.

Expected: Ledger bytes and reservations match the service, custom files remain outside the measurement, callback errors stay sanitized, and no setting is changed by reading.

Forbidden: Native GUI or file-manager launch, user-file deletion, whole-disk usage claims, stale success after a failed refresh, or missing checksummed evidence.

```text
acceptance desktopui settings-storage
```

## settings-visual-evidence

Native settings screenshots for the parallel settings delivery

Steps: Start an isolated production desktop, visit each implemented settings section, reveal its controls and retain native window captures.

Expected: Settings controls render in the live composition; the real service supplies diagnostic and cache values; screenshots and a continuous recording are retained.

Forbidden: Personal profiles, system clipboard access, telemetry uploads, permission requests, file dialogs, invented pixels or claims of completed update/paste acceptance.

```text
start
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 通用
screenshot general
ui-wait listItem 诊断与隐私
ui-click listItem 诊断与隐私
ui-wait popUpButton 本机日志等级
ui-click button 刷新诊断状态
ui-enabled button 刷新诊断状态
screenshot diagnostics-levels
ui-reveal button 打开日志目录
screenshot diagnostics-signals-export
ui-click listItem 存储与备份
ui-wait button 刷新缓存用量
ui-click button 刷新缓存用量
ui-enabled button 刷新缓存用量
screenshot storage-usage
ui-reveal button 导入配置
screenshot backup-controls
ui-click listItem 外观
ui-wait button 深色
screenshot appearance
ui-click button 深色
config-equals desktop.color_mode '"dark"'
observe 1
screenshot appearance-dark
ui-click button 跟随系统
config-equals desktop.color_mode '"system"'
ui-click listItem 快捷面板
ui-wait button 刷新权限状态
ui-enabled button 刷新权限状态
screenshot shortcut-permissions
```

## setup-later-migration

Settings migration cancellation and restart

Steps: Explicitly postpone first-run migration; open migration from Settings; unlock and cancel actual import; quit and reopen the same profile; resume import; enter history; reopen again.

Expected: Settings stops its owned service; pending migration survives restart; completion alone leaves service stopped; explicit entry starts service; all 1505 records persist without duplication after another restart.

Forbidden: Personal data or credentials, system clipboard access, implicit completion after cancellation, active service during import, missing recordings or source mutation.

```text
start-setup complete
ui-wait text '欢迎使用 UniClipboard'
ui-click button '开始使用'
ui-wait button '暂不迁移'
ui-click button '暂不迁移'
history-count 0
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 存储与备份
ui-click listItem 存储与备份
ui-wait button '从 v1 迁移'
ui-reveal button '从 v1 迁移'
screenshot settings
ui-click button '从 v1 迁移'
ui-wait button '开始迁移'
service-absent
screenshot later-confirmation
ui-click button '开始迁移'
ui-wait textField 'v1 口令'
ui-set textField 'v1 口令' fixture-passphrase
ui-enabled button '解锁并继续'
ui-click button '解锁并继续'
ui-wait text '正在导入记录'
ui-click button '停止迁移'
ui-wait text '迁移已停止'
service-absent
screenshot cancelled
restart-desktop setup
ui-wait button '开始迁移'
service-absent
screenshot reopened-pending
ui-click button '开始迁移'
ui-wait textField 'v1 口令'
ui-set textField 'v1 口令' fixture-passphrase
ui-enabled button '解锁并继续'
ui-click button '解锁并继续'
ui-wait text '迁移完成' 90
service-absent
screenshot complete
ui-click button '开始使用'
history-count 1505
screenshot history
restart-desktop
history-count 1505
ui-wait listItem '~.*fixture text.*'
screenshot persisted-history
```

## setup-onboarding

Native first-install onboarding and complete migration

Steps: Start with 1505 encrypted synthetic records; verify no service before consent; submit a wrong passphrase then the correct fixture passphrase; observe import and verification; enter history and count all migrated records.

Expected: Welcome copy matches the decision; secrets stay masked; import and verification remain distinct; migration completes with 1505 records visible through production IPC; every page has a screenshot and a continuous recording.

Forbidden: Personal profiles, system credential access, actual clipboard access, fixture data linked into release binaries, fabricated progress, or missing video frames after resizing.

```text
start-setup complete
ui-wait text '欢迎使用 UniClipboard'
ui-wait text '让每一次复制更有用'
observe 2
screenshot welcome
service-absent
ui-click button '开始使用'
ui-wait button '开始迁移'
observe 3
screenshot confirmation
service-absent
ui-click button '开始迁移'
ui-wait textField 'v1 口令'
observe 2
screenshot passphrase
ui-set textField 'v1 口令' wrong-passphrase
ui-enabled button '解锁并继续'
ui-click button '解锁并继续'
ui-wait alert '无法解锁旧数据。请核对 v1 口令后重试。'
observe 2
screenshot wrong-passphrase
ui-set textField 'v1 口令' fixture-passphrase
ui-enabled button '解锁并继续'
ui-click button '解锁并继续'
ui-wait text '正在导入记录'
screenshot importing
ui-wait text '正在校验导入的内容' 90
screenshot verifying
ui-wait text '迁移完成' 90
observe 3
screenshot complete
service-absent
ui-click button '开始使用'
history-count 1505
ui-wait listItem '~.*fixture text.*'
observe 3
screenshot history
```

## setup-pages

First-install setup pages

Steps: Render welcome, detection error, confirmation, invalid source, passphrase, conflict, import, verification, stopping and each terminal result; activate primary and secondary actions.

Expected: Agreed welcome copy, masked input, separate progress phases and report counts, actionable errors, correct buttons and screenshots for each exercised view.

Forbidden: Native window launch, credential access, personal data access, partial results labeled complete, or missing view evidence.

```text
acceptance gui setup-pages
```

## setup-partial

Native first-install partial migration

Steps: Start with an encrypted source containing one unrecoverable record; unlock through the fixture passphrase; observe the partial report; enter history and count 1504 imported records.

Expected: The result says some data could not be imported; the failed-record count is separate; entry into history remains available; the real service exposes exactly 1504 records; continuous video is retained.

Forbidden: Complete-success title for partial import, personal credentials, system clipboard access, fabricated progress, or unavailable retained recording.

```text
start-setup partial
ui-wait text '欢迎使用 UniClipboard'
observe 2
screenshot welcome
ui-click button '开始使用'
ui-wait button '开始迁移'
observe 2
screenshot confirmation
ui-click button '开始迁移'
ui-wait textField 'v1 口令'
ui-set textField 'v1 口令' fixture-passphrase
ui-enabled button '解锁并继续'
ui-click button '解锁并继续'
ui-wait text '正在导入记录'
screenshot importing
ui-wait text '正在校验导入的内容' 90
screenshot verifying
ui-wait text '部分数据未能导入' 90
ui-wait text '失败记录'
ui-absent button '开始使用'
observe 4
screenshot partial
service-absent
ui-click button '进入应用'
history-count 1504
ui-wait listItem '~.*fixture text.*'
observe 3
screenshot history
```

## sheet-exit-stability

Sheet exit stability

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Light and dark exit rasters preserve stable content and release dismissed input.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui sheet-exit-stability
```

## sheet-motion

Sheet motion

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Real Sheets preserve entry geometry, centered resizing and completed exits; inline tags use no Sheet, Enter attaches, and Escape/click-away cancel without mutation across all theme/motion combinations.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui sheet-motion
```

## sidebar-collapse

Sidebar collapse

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Animated content geometry remains continuous and preserves sidebar state.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui sidebar-collapse
```

## sidebar-navigation

Sidebar navigation paint, selection and scrollbar clearance

Steps: Hover a tag; select tag/all; scroll overflowing devices; inspect state, pixels and bounds in both schemes at 1x/2x.

Expected: Hover does not change the filter; one selected row; different state paint; 12-DIP gutter; footer stays reachable.

Forbidden: Fake state, a scrollbar over counts, hidden native controls, or native OS acceptance claims.

```text
acceptance desktopui sidebar-navigation
```

## source-application

Source application in history detail

Steps: Project service history metadata, render the complete history view in both BoardUI themes, and switch records.

Expected: Cached icons render; absent or malformed icons use a generic icon; absent names use the identifier; long names remain bounded.

Forbidden: Stale source metadata after selection changes, replacement by local application identity, hidden device/time, or native-window claims from software-view evidence.

```text
acceptance desktopui source-application
```

## startup-lifecycle

Desktop startup and service lifetime

Steps: Assert fresh CLI refusal, explicitly initialize both private profiles, then execute the production-process suite and validate its receipt.

Expected: The suite runs, passes its assertions and emits a successful receipt.

Forbidden: A skipped suite, a missing receipt, or labeling fixtures as native OS acceptance.

```text
acceptance startup startup-lifecycle
```

## storage-cleanup-settings

Native cleanup control state and persistence

Steps: Open real storage settings, verify disabled rules, enable cleanup, disable it again, restart and reopen.

Expected: Age/count/tag controls follow the cleanup switch; cache quota stays enabled; the saved choice survives restart.

Forbidden: Active cleanup-rule controls while cleanup is off, lost settings, or claims of native file deletion coverage.

```text
start
submit text 'UNIC-D cleanup settings synthetic'
ui-wait listItem 'UNIC-D cleanup settings synthetic'
ui-click listItem 设置…
ui-wait listItem 存储与备份
ui-click listItem 存储与备份
config-equals history.auto_cleanup false
ui-disabled popUpButton 历史保留时间
ui-disabled popUpButton 最大历史条目数
ui-disabled button '保留 #文本'
ui-enabled popUpButton 单设备缓存配额
screenshot cleanup-off
ui-enabled switch 自动清理历史
ui-click switch 自动清理历史
config-equals history.auto_cleanup true
ui-enabled popUpButton 历史保留时间
ui-enabled popUpButton 最大历史条目数
ui-enabled button '保留 #文本'
ui-enabled switch 自动清理历史
ui-click switch 自动清理历史
config-equals history.auto_cleanup false
restart-desktop
ui-wait listItem 设置…
ui-click listItem 设置…
ui-wait listItem 存储与备份
ui-click listItem 存储与备份
config-equals history.auto_cleanup false
ui-disabled popUpButton 历史保留时间
ui-disabled popUpButton 最大历史条目数
ui-disabled button '保留 #文本'
ui-enabled popUpButton 单设备缓存配额
screenshot persisted-after-restart
```

## system-clipboard-formats

Real guest HTML, PNG and file clipboard roundtrip

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Offer synthetic HTML, PNG and a task-owned file through wl-copy; read production history; replace the offer; select its real history row and restore through physical Enter; read wl-paste.

Expected: Exact payload bytes survive capture and restore; each offer creates one history entry; restore produces no echo; the original file remains intact.

Forbidden: Directory clipboard substitution, host clipboard access, target insertion claims or directory-transfer acceptance claims.

```text
system-board-write 'UNIC-D formats startup baseline'
start
system-format-roundtrip html
system-format-roundtrip png
system-format-roundtrip file
history-count 3
screenshot formats-restored-without-echo
```

## system-clipboard-receive

Production peer to real guest clipboard

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Enable sync through settings; pair an owned production peer with fingerprint approvals; submit synthetic peer text and read the real guest clipboard and remote history.

Expected: Native clipboard and remote history contain the exact received text; sender history gains only its submitted item; both owned services exit at cleanup.

Forbidden: Claims of two graphical endpoints, shared clipboard propagation, skipped approvals or directory clipboard substitution.

```text
system-board-write 'UNIC-D receive startup baseline'
start
ui-click listItem 设置…
ui-wait listItem 同步
ui-click listItem 同步
ui-enabled switch 启用同步
ui-click switch 启用同步
config-equals sync.enabled true
connect-policy-peer
system-peer-text 'UNIC-D received native 中文'
screenshot real-clipboard-received
```

## system-clipboard-target

Physical paste into a real target application

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Apply a synthetic history item through physical Enter; start an isolated GNOME Text Editor; paste, read inserted text and save through physical guest keys.

Expected: The owned editor has OS focus, its accessible text matches exactly, and its saved document matches the fixture including GtkSourceView's default trailing newline.

Forbidden: Host clipboard access, personal editor state, keys sent without verified focus, or treating dispatch alone as insertion.

```text
start
submit target 'UNIC-D native target 中文'
ui-wait listItem 'UNIC-D native target 中文'
ui-click listItem 'UNIC-D native target 中文'
system-key Enter
system-board-equals 'UNIC-D native target 中文'
screenshot applied-before-target
system-paste-target 'UNIC-D native target 中文' expected-target.txt

-- expected-target.txt --
UNIC-D native target 中文
```

## system-clipboard

Real guest clipboard capture and application

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Set a synthetic guest baseline, start Desktop, capture a fresh copy, restore another item and observe the clipboard.

Expected: Startup does not capture the baseline; a fresh copy enters history; restore changes the real clipboard once without echo.

Forbidden: Host clipboard access, shared-clipboard propagation, a directory clipboard, or claims of cross-application insertion.

```text
system-board-write 'UNIC-D startup baseline'
start
history-count 0
system-board-equals 'UNIC-D startup baseline'
observe 2
history-count 0
system-board-write 'UNIC-D native capture 中文'
ui-wait listItem 'UNIC-D native capture 中文'
history-count 1
submit restore 'UNIC-D native restored 中文'
ui-wait listItem 'UNIC-D native restored 中文'
ui-click listItem 'UNIC-D native restored 中文'
system-key Enter
system-board-equals 'UNIC-D native restored 中文'
observe 2
history-count 2
screenshot restored-without-echo
```

## system-linux-ui-dark

Native Linux narrow window in dark appearance

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Open a 600x480 native Linux window; submit synthetic Chinese text; focus search through physical Ctrl+F; filter; open and dismiss the production command menu.

Expected: Chinese history and matching search remain accessible; the real command menu opens and Escape dismisses it; screenshots and native recording retain the actual composition.

Forbidden: IME claims from semantic field values, foreign skin overrides, software-view evidence or host clipboard access.

```text
system-board-write 'UNIC-D Linux UI baseline'
start 600 480 dark
submit ui 'UNIC-D 原生中文布局验证'
ui-wait listItem 'UNIC-D 原生中文布局验证'
screenshot narrow-history
system-key Ctrl+F
ui-wait comboBox 搜索历史
ui-set comboBox 搜索历史 中文布局
ui-wait listItem 'UNIC-D 原生中文布局验证'
screenshot narrow-search
ui-set comboBox 搜索历史 ''
system-key Escape
system-key Escape
ui-click menuButton 更多操作
screenshot native-command-menu
system-key Escape
history-count 1
```

## system-linux-ui-light

Native Linux narrow window in light appearance

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Open a 600x480 native Linux window; submit synthetic Chinese text; focus search through physical Ctrl+F; filter; open and dismiss the production command menu.

Expected: Chinese history and matching search remain accessible; the real command menu opens and Escape dismisses it; screenshots and native recording retain the actual composition.

Forbidden: IME claims from semantic field values, foreign skin overrides, software-view evidence or host clipboard access.

```text
system-board-write 'UNIC-D Linux UI baseline'
start 600 480 light
submit ui 'UNIC-D 原生中文布局验证'
ui-wait listItem 'UNIC-D 原生中文布局验证'
screenshot narrow-history
system-key Ctrl+F
ui-wait comboBox 搜索历史
ui-set comboBox 搜索历史 中文布局
ui-wait listItem 'UNIC-D 原生中文布局验证'
screenshot narrow-search
ui-set comboBox 搜索历史 ''
system-key Escape
system-key Escape
ui-click menuButton 更多操作
screenshot native-command-menu
system-key Escape
history-count 1
```

## system-managed-cleanup

Native clipboard holder and excluded-tag cleanup

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Pair an owned production peer; receive a synthetic user file onto the real clipboard; enable count cleanup with an excluded tag; remove the exclusion; replace the clipboard.

Expected: Exclusion protects history and file; removing it deletes eligible history but preserves the held native file; replacing the clipboard reclaims only the managed file and preserves the original source.

Forbidden: Directory fixtures, deleting a user-owned source, treating history deletion as holder release, or claims of two graphical endpoints.

```text
system-board-write 'UNIC-D managed cleanup baseline'
start
ui-click listItem 设置…
ui-wait listItem 同步
ui-click listItem 同步
ui-enabled switch 启用同步
ui-click switch 启用同步
config-equals sync.enabled true
connect-policy-peer
system-file-cleanup
screenshot native-holder-released
```

## system-offline-backlog

Offline received history must not auto-apply on Desktop startup

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Pair an owned peer; quit Desktop; run the same profile as a standalone service; receive text; reopen normal Desktop; then deliver fresh text.

Expected: The standalone service receives history without touching the clipboard; Desktop reuses that independent service, retains history and preserves the existing clipboard baseline; fresh text subsequently applies without echo.

Forbidden: Old backlog auto-application, an uninitialized profile, discarded history, parallel clipboard owners or host clipboard access.

```text
system-board-write 'UNIC-D offline startup baseline'
start
ui-click listItem 设置…
ui-wait listItem 同步
ui-click listItem 同步
ui-enabled switch 启用同步
ui-click switch 启用同步
config-equals sync.enabled true
connect-policy-peer
screenshot before-offline-service
system-offline-backlog
screenshot reopened-after-offline-backlog
```

## system-second-launch

Native second launch recreates the GUI process

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Close the focused task main window through physical Alt+F4; launch the same application identity; inspect the new GUI process and service generation.

Expected: The second process exits successfully; the service generation remains and a new GUI process opens; fresh capture works after reopening; explicit Quit cleans the GUI, tray and owned service.

Forbidden: Claims that reopening proves capture while the GUI process is absent, unrelated application activation or duplicate service owners.

```text
system-board-write 'UNIC-D second launch baseline'
start
system-key Alt+F4
# Main-window close now means that the GUI process has exited.
system-window unic hidden
system-second-launch
system-window unic visible
system-board-write 'UNIC-D second launch reopened 中文'
ui-wait listItem 'UNIC-D second launch reopened 中文'
history-count 1
screenshot recreated-gui
```

## system-wayland-panel

Native Wayland quick-panel Copy fallback

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Open the production panel from settings, attempt Enter, copy through Ctrl+C, reopen and dismiss through Escape.

Expected: The panel falls back to Copy without an inline alert banner; Enter leaves the clipboard unchanged; Copy applies exact bytes; Escape hides the panel without changing the clipboard.

Forbidden: Synthetic target capture authority, automatic paste claims, portal approval claims or host clipboard access.

```text
system-board-write 'UNIC-D panel baseline'
start
submit panel 'UNIC-D panel copy 中文'
ui-wait listItem 'UNIC-D panel copy 中文'
ui-click listItem 设置…
ui-wait listItem 快捷面板
ui-click listItem 快捷面板
ui-wait button 打开快捷面板
ui-click button 打开快捷面板
system-window 'unic · 快捷面板' visible
ui-wait text 复制到剪贴板
ui-absent alert '~.*'
screenshot wayland-copy-fallback
system-key Enter
system-board-equals 'UNIC-D panel baseline'
system-key Ctrl+C
system-board-equals 'UNIC-D panel copy 中文'
system-window 'unic · 快捷面板' hidden
system-window unic visible
ui-click button 打开快捷面板
system-window 'unic · 快捷面板' visible
system-key Escape
system-window 'unic · 快捷面板' hidden
system-board-equals 'UNIC-D panel copy 中文'
system-window unic visible
history-count 1
screenshot panel-dismissed
```

## system-window-lifecycle

Real guest close, background capture and second launch

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Close the focused owned main window through physical Alt+F4, offer fresh text while the GUI process is absent, launch the same application identity again, then reopen history and quit at cleanup.

Expected: The tray retains its service and captures exact fresh content after the GUI exits; second launch exits and preserves the service generation; the new GUI process becomes visible; explicit Quit reaps the owner and service.

Forbidden: Closing unrelated applications, replacing the active service, or treating a process fixture as a native close.

```text
system-board-write 'UNIC-D lifecycle startup baseline'
start
system-key Alt+F4
# Main-window close now means that the GUI process has exited.
system-window unic hidden
system-board-write 'UNIC-D capture while GUI absent 中文'
history-count 1
system-second-launch
system-window unic visible
ui-wait listItem 'UNIC-D capture while GUI absent 中文'
screenshot recreated-gui
```

## tag-editor

V1 inline tag editor and colors

Steps: Open production history composition in BoardUI and both themes; create, recolor, attach, remove, cancel and use keyboard navigation.

Expected: Inline ComboField and palette preserve focus, names, associations and shared color state; rendered checkpoints are retained.

Forbidden: Checkbox assignment Sheet, duplicate creation, mutation on color choice or cancel, skipped subcases or native claims.

```text
acceptance desktopui tag-editor
```

## tag-fixed-columns

Tag fixed columns

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Complete management sheets preserve column alignment and row geometry.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui tag-fixed-columns
```

## tag-manager-platforms

BoardUI tag management and assignment

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Built-in operations remain protected and assignment updates the shared registry in both themes.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui tag-manager-platforms
```

## tag-manager

Tag manager

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: All original management operations preserve their history and shared tag-registry invariants.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui tag-manager
```

## token-search

Token search

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Fourteen scenarios in BoardUI and both themes preserve the complete search contract.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui token-search
```

## toolbar-alignment

Toolbar alignment

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: A 52-point band contains layout, search and command controls with vertical insets; expanded search stays within 600, 800, 1100 and 1440-point windows; each light-theme BoardUI control remains visibly distinct from the toolbar background.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui toolbar-alignment
```

## toolbar-bounds-dark-1100

Native toolbar bounds at 1100 points in dark mode

Steps: Start an isolated production desktop; focus the always-visible search, edit and clear its query, switch layouts and open the command menu.

Expected: Every toolbar control remains inside the band; search and menus remain usable.

Forbidden: Real profile access, system clipboard writes, missing native recording, or claims about OS decorations.

```text
start 1100 720 dark
submit entry 'Toolbar bounds evidence'
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot idle
ui-click comboBox '搜索历史'
ui-wait comboBox '搜索历史'
ui-toolbar-contained
ui-set comboBox '搜索历史' bounds
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot query
ui-set comboBox '搜索历史' ''
ui-key Escape
ui-key Escape
ui-click toggleButton '网格'
ui-toolbar-contained
screenshot grid
ui-click menuButton '更多操作'
screenshot menu
ui-key Escape
```

## toolbar-bounds-dark-600

Native toolbar bounds at 600 points in dark mode

Steps: Start an isolated production desktop; focus the always-visible search, edit and clear its query, switch layouts and open the command menu.

Expected: Every toolbar control remains inside the band; search and menus remain usable.

Forbidden: Real profile access, system clipboard writes, missing native recording, or claims about OS decorations.

```text
start 600 480 dark
submit entry 'Toolbar bounds evidence'
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot idle
ui-click comboBox '搜索历史'
ui-wait comboBox '搜索历史'
ui-toolbar-contained
ui-set comboBox '搜索历史' bounds
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot query
ui-set comboBox '搜索历史' ''
ui-key Escape
ui-key Escape
ui-click toggleButton '网格'
ui-toolbar-contained
screenshot grid
ui-click menuButton '更多操作'
screenshot menu
ui-key Escape
```

## toolbar-bounds-light-1100

Native toolbar bounds at 1100 points in light mode

Steps: Start an isolated production desktop; focus the always-visible search, edit and clear its query, switch layouts and open the command menu.

Expected: Every toolbar control remains inside the band; search and menus remain usable.

Forbidden: Real profile access, system clipboard writes, missing native recording, or claims about OS decorations.

```text
start 1100 720 light
submit entry 'Toolbar bounds evidence'
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot idle
ui-click comboBox '搜索历史'
ui-wait comboBox '搜索历史'
ui-toolbar-contained
ui-set comboBox '搜索历史' bounds
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot query
ui-set comboBox '搜索历史' ''
ui-key Escape
ui-key Escape
ui-click toggleButton '网格'
ui-toolbar-contained
screenshot grid
ui-click menuButton '更多操作'
screenshot menu
ui-key Escape
```

## toolbar-bounds-light-600

Native toolbar bounds at 600 points in light mode

Steps: Start an isolated production desktop; focus the always-visible search, edit and clear its query, switch layouts and open the command menu.

Expected: Every toolbar control remains inside the band; search and menus remain usable.

Forbidden: Real profile access, system clipboard writes, missing native recording, or claims about OS decorations.

```text
start 600 480 light
submit entry 'Toolbar bounds evidence'
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot idle
ui-click comboBox '搜索历史'
ui-wait comboBox '搜索历史'
ui-toolbar-contained
ui-set comboBox '搜索历史' bounds
ui-wait listItem 'Toolbar bounds evidence'
ui-toolbar-contained
screenshot query
ui-set comboBox '搜索历史' ''
ui-key Escape
ui-key Escape
ui-click toggleButton '网格'
ui-toolbar-contained
screenshot grid
ui-click menuButton '更多操作'
screenshot menu
ui-key Escape
```

## tray-icon-settings

Tray icon styles saved and applied

Steps: Use the production settings view, real service/CLI, persisted TOML, configuration watcher and tray image controller; reopen and reject invalid values.

Expected: Glyph is the default; both styles persist and apply without restart; duplicate updates do not publish images; failed delivery retries.

Forbidden: Native GUI launch, fixture-only configuration, style changes rebuilding tray menus, or claiming OS-tray acceptance.

```text
acceptance desktopui tray-icon-settings
```

## unified-scrollbars

Unified scrollbars

Steps: Execute the registered scenario and all its BoardUI scheme and interaction subcases.

Expected: Three skins and both themes preserve edge indicators and independent nested scroll ownership.

Forbidden: Skipped subcases, lost assertions, or software-view images labeled as native evidence.

```text
acceptance desktopui unified-scrollbars
```

## wetype-pairing

WeType shared-space pairing from Desktop

Steps: Open the production plugin detail with a local service fixture, check the pending-restart and short-code states, reject one code, then accept a new code and show the joined space.

Expected: The request selects the default instance, the user sees useful rejection feedback, and the joined space replaces the form after refresh.

Forbidden: Instance IDs in the first-run form, requests before restart or for incomplete codes, repeated submission while pending, and a native or live WeType acceptance claim.

```text
acceptance desktopui wetype-pairing
```

## window-chrome-native-dark

Native dark main-window frame persistence and GUI recreation

Steps: Save the frame preference through General settings, retain the existing window, close GUI, reopen through tray, and repeat in the opposite direction with the same service and history.

Expected: The same boolean persists; the service has no host-only restart entry; a new GUI clears pending; Windows shows exactly one set of caption buttons for its mode.

Forbidden: Service restart, second caption row, system clipboard access, physical-input claims, or using window-content screenshots as proof of OS decorations.

```text
start 600 720 dark native
submit chrome-native 'Windows chrome synthetic history'
ui-wait listItem 'Windows chrome synthetic history'
role-checkpoint initial
[windows] ui-wait button '最小化'
screenshot custom-history
[windows] windows-frame custom-history-os custom
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait button '切换设置侧栏'
ui-click button '切换设置侧栏'
ui-wait listItem '外观'
ui-click listItem '外观'
ui-wait switch '使用系统边框'
ui-reveal switch '使用系统边框'
ui-click switch '使用系统边框'
config-equals desktop.system_window_frame true
restart-clear
ui-wait text '部分设置将在下次启动时生效。'
ui-reveal text '部分设置将在下次启动时生效。'
[windows] ui-wait button '最小化'
screenshot saved-pending
role-close-gui
role-reopen-gui
ui-wait listItem 'Windows chrome synthetic history'
[windows] ui-absent button '最小化'
[windows] ui-absent button '最大化'
[windows] ui-absent button '关闭'
screenshot system-history
[windows] windows-frame system-history-os system
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait button '切换设置侧栏'
ui-click button '切换设置侧栏'
ui-wait listItem '外观'
ui-click listItem '外观'
ui-wait switch '使用系统边框'
ui-absent text '部分设置将在下次启动时生效。'
ui-reveal switch '使用系统边框'
ui-click switch '使用系统边框'
config-equals desktop.system_window_frame false
restart-clear
ui-wait text '部分设置将在下次启动时生效。'
ui-reveal text '部分设置将在下次启动时生效。'
role-close-gui
role-reopen-gui
ui-wait listItem 'Windows chrome synthetic history'
[windows] ui-wait button '最小化'
screenshot custom-reopened
[windows] windows-frame custom-reopened-os custom
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait button '切换设置侧栏'
ui-click button '切换设置侧栏'
ui-wait listItem '外观'
ui-click listItem '外观'
ui-wait switch '使用系统边框'
ui-absent text '部分设置将在下次启动时生效。'

[windows] ui-click button '最大化'
[windows] windows-frame maximized-os custom maximized
[windows] ui-click button '还原'
[windows] windows-frame restored-os custom restored
[windows] windows-frame minimized-restored-os custom minimized

[windows] ui-click button '切换设置侧栏'
[windows] ui-wait listItem '插件'
[windows] ui-click listItem '插件'
[windows] ui-wait button '内容分类'
[windows] ui-click button '内容分类'
[windows] ui-wait button '返回插件列表'
[windows] ui-wait button '关闭'
[windows] windows-frame plugin-detail-os custom
[windows] role-close-gui caption
[windows] role-reopen-gui
[windows] ui-wait listItem 'Windows chrome synthetic history'
```

## window-chrome-native-input

Native Windows toolbar drag and resize

Environment: disposable guest; explicit GUEST=<VM UUID> required.

Steps: Open General through the existing sidebar, drag empty toolbar space, resize the native edge and double-click empty toolbar space.

Expected: OS geometry changes, IsZoomed confirms maximize, caption close leaves tray/service alive, and tray command reopens history.

Forbidden: Personal profiles, broad process termination, clipboard access, or claiming Windows Snap acceptance.

```text
[!windows] skip 'Windows physical window input only'
start 640 560 light native
submit chrome-input 'Windows chrome input synthetic history'
ui-wait listItem 'Windows chrome input synthetic history'
role-checkpoint initial
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait switch '登录时启动'
windows-frame physical-drag-resize-os custom drag-resize
role-close-gui caption
role-reopen-gui
ui-wait listItem 'Windows chrome input synthetic history'
windows-frame reopened-history-os custom
```

## window-chrome-native

Native main-window frame persistence and GUI recreation

Steps: Save the frame preference through General settings, retain the existing window, close GUI, reopen through tray, and repeat in the opposite direction with the same service and history.

Expected: The same boolean persists; the service has no host-only restart entry; a new GUI clears pending; Windows shows exactly one set of caption buttons for its mode.

Forbidden: Service restart, second caption row, system clipboard access, physical-input claims, or using window-content screenshots as proof of OS decorations.

```text
start 640 720 light native
submit chrome-native 'Windows chrome synthetic history'
ui-wait listItem 'Windows chrome synthetic history'
role-checkpoint initial
[windows] ui-wait button '最小化'
screenshot custom-history
[windows] windows-frame custom-history-os custom
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait button '切换设置侧栏'
ui-click button '切换设置侧栏'
ui-wait listItem '外观'
ui-click listItem '外观'
ui-wait switch '使用系统边框'
ui-reveal switch '使用系统边框'
ui-click switch '使用系统边框'
config-equals desktop.system_window_frame true
restart-clear
ui-wait text '部分设置将在下次启动时生效。'
ui-reveal text '部分设置将在下次启动时生效。'
[windows] ui-wait button '最小化'
screenshot saved-pending
role-close-gui
role-reopen-gui
ui-wait listItem 'Windows chrome synthetic history'
[windows] ui-absent button '最小化'
[windows] ui-absent button '最大化'
[windows] ui-absent button '关闭'
screenshot system-history
[windows] windows-frame system-history-os system
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait button '切换设置侧栏'
ui-click button '切换设置侧栏'
ui-wait listItem '外观'
ui-click listItem '外观'
ui-wait switch '使用系统边框'
ui-absent text '部分设置将在下次启动时生效。'
ui-reveal switch '使用系统边框'
ui-click switch '使用系统边框'
config-equals desktop.system_window_frame false
restart-clear
ui-wait text '部分设置将在下次启动时生效。'
ui-reveal text '部分设置将在下次启动时生效。'
role-close-gui
role-reopen-gui
ui-wait listItem 'Windows chrome synthetic history'
[windows] ui-wait button '最小化'
screenshot custom-reopened
[windows] windows-frame custom-reopened-os custom
ui-click button '切换侧栏'
ui-wait listItem '设置…'
ui-click listItem '设置…'
ui-wait button '切换设置侧栏'
ui-click button '切换设置侧栏'
ui-wait listItem '外观'
ui-click listItem '外观'
ui-wait switch '使用系统边框'
ui-absent text '部分设置将在下次启动时生效。'

[windows] ui-click button '最大化'
[windows] windows-frame maximized-os custom maximized
[windows] ui-click button '还原'
[windows] windows-frame restored-os custom restored
[windows] windows-frame minimized-restored-os custom minimized

[windows] ui-click button '切换设置侧栏'
[windows] ui-wait listItem '插件'
[windows] ui-click listItem '插件'
[windows] ui-wait button '内容分类'
[windows] ui-click button '内容分类'
[windows] ui-wait button '返回插件列表'
[windows] ui-wait button '关闭'
[windows] windows-frame plugin-detail-os custom
[windows] role-close-gui caption
[windows] role-reopen-gui
[windows] ui-wait listItem 'Windows chrome synthetic history'
```

## window-chrome-setup

Native setup and migration caption controls

Steps: Open isolated first-install setup, enter migration confirmation and the synthetic passphrase fallback, and exercise caption actions.

Expected: Windows caption controls share the window top with the setup panels; the existing migration controls remain usable; the service remains absent before consent.

Forbidden: Personal profiles, real credential access, clipboard changes, an extra normal-page caption row, or native-frame claims from content captures.

```text
start-setup complete
ui-wait text '欢迎使用 UniClipboard'
service-absent
screenshot setup-welcome
[windows] windows-frame setup-welcome-os custom
ui-click button '开始使用'
ui-wait button '开始迁移'
screenshot setup-confirmation
[windows] windows-frame setup-confirmation-os custom
ui-click button '开始迁移'
ui-wait textField 'v1 口令'
screenshot setup-passphrase
[windows] windows-frame setup-passphrase-os custom
[windows] ui-click button '最大化'
[windows] windows-frame setup-maximized-os custom maximized
[windows] ui-click button '还原'
[windows] ui-wait button '最大化'
[windows] windows-frame setup-minimized-os custom minimized
```

## window-chrome-startup-error

Native startup-error window controls

Steps: Create an owned directory at the database path, launch a completed private profile, and inspect the resulting retry page and its fallback controls.

Expected: A real startup failure leaves a visible retry page with one draggable fallback band and functioning caption actions.

Forbidden: Personal data, a fake success screen, missing controls, system clipboard access, or OS frame claims based on content capture.

```text
start-error
ui-wait button '重试' 60
screenshot startup-error
[windows] windows-frame startup-error-os custom
[windows] ui-click button '最大化'
[windows] windows-frame startup-error-maximized-os custom maximized
[windows] ui-click button '还原'
[windows] ui-wait button '最大化'
[windows] windows-frame startup-error-minimized-os custom minimized
```

## window-chrome

Main-window decoration and shared-toolbar caption actions

Steps: On macOS also exercise the integrated native traffic-light band; on every host exercise complete history/settings/plugin views at 600/640/1100 widths in light/dark themes and scaled rendering; check geometry and dispatch caption actions.

Expected: The preference defaults off; either native or custom controls are shown; custom controls share the history toolbar and preserve search and commands.

Forbidden: A second caption row on ordinary pages, overlap, lost resize edges, duplicate buttons, or claims of native OS interaction.

```text
acceptance desktopui window-chrome
```
