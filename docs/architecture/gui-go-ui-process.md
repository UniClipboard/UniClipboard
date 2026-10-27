# Desktop UI process lifecycle

## 进程所有权与运行模式

正常模式保留一个桌面宿主和 daemon，按需创建一个界面子进程。宿主持有单实例锁，负责托盘、原生快捷面板、全局快捷键、登录项、通知和更新调度，本身不创建 WebView。子进程通过继承的私有管道连接宿主，只连接已有 daemon，负责主窗口、更新窗口与 WebView 回退面板。

关闭主窗口通常退出界面子进程，保留正常模式宿主与 daemon。若 WebView 回退面板仍在显示，则由 Wails 关闭并销毁主窗口，保留面板自己的资源；收起最后一个界面才退出子进程。因此保证的是主窗口拥有的 WebView 释放，不是存在回退面板时所有浏览器辅助进程退出。原生面板的进程与此独立。

轻量模式手动打开应用时，临时宿主同时显示托盘和主窗口，以访问设置与历史，但不启动快捷面板或注册全局快捷键。子进程退出或崩溃并被回收后，临时宿主与托盘一起退出，daemon 不停止。关闭窗口不修改模式。轻量模式可保存快捷键与面板偏好，但不会注册它们；恢复正常模式时应用这些偏好。

已有托盘的宿主直接应用模式变更，保留宿主、界面子进程和主窗口；正常模式激活快捷面板及快捷键，轻量模式释放这些运行资源。执行器在启动时只构造一次，轻量模式不启动面板进程或注册快捷键。托盘的“轻量模式”动作先关闭当前界面，设置监听器应用轻量驻留后退出空闲宿主，最终仅保留 daemon。仅缺少托盘的后台宿主需要通过既有重启路径配置托盘。新宿主等待旧宿主退出后竞争同一单实例锁；它重新读取最新设置。退出意图只能提交一次，后续子进程回收不会覆盖完整退出或已成功启动的重启决定。跨越关闭的显式激活等待旧子进程回收；跨越宿主退出的显式激活交给等待旧宿主退出的新启动处理。

## 启动来源与平台边界

`general.startupMode` 表示正常或轻量驻留；`general.backgroundStartup` 只控制自动登录启动时的主窗口。首次手动打开即使启动了 daemon，也显示主窗口。重复自动登录启动不打开已有实例的窗口；重复显式启动显示主窗口。旧版设置的兼容投影见 ADR-013。

Windows、Linux 与 macOS LaunchAgent 使用 `--autostart`。Wails beta.28 的 SMAppService 忽略启动参数且只提供注册状态，因此 macOS 在原生启动完成通知中同步记录 AppKit 的 `keyAELaunchedAsLogInItem`，Go 启动回调再从主线程读取。实际登录验收必须在 macOS 上执行，不能用传入参数的测试代替。

Wails beta.28 在运行中的 `SystemTray.New` 内立即调用 `Run`，Linux 只在该次 `Run` 创建 `Menu` 属性。因此仍需在应用事件循环前配置托盘。显式启动在事件循环之前配置托盘。后台启动在单实例锁之前只读探测已有 daemon 的设置，不启动或修改 daemon；如果启动后才确定需要托盘，则先完成 daemon 恢复，再通过现有重启交接给具备托盘配置的宿主。单实例锁之后、事件循环之前不等待 daemon 启动，重复激活仍可被接收。交接提示仅用于内部重启且不覆盖成功读取的设置。

macOS 子进程位于 `Contents/Helpers/UniClipboardUI.app`，使用独立 `<bundle-id>.ui` 标识；Windows 和 Linux 通过内部 `--ui-child` 参数复用同一程序。没有继承管道的直接子进程启动仍被拒绝。Linux 两个角色使用独立的 GTK 应用标识，AppImage 子进程继承挂载环境；整应用重启仍通过外层 AppImage。未更改 daemon 的配置根、密钥、内容或同步协议。

正常模式无窗口时继续检查和下载更新，但计划提示不打开子进程。轻量模式仅剩 daemon 时不运行桌面更新调度、托盘通知或快捷键。所有模式的完整退出仍停止 daemon，模式交接与普通窗口关闭不会停止 daemon。

## Why window Close was insufficient

Wails Close removes the native window and registry entry. It does not guarantee that all WebKit
auxiliary processes exit. WebKit's default data store may outlive the last view. Apple also states
that multiple WKProcessPool objects no longer affect process allocation. The new UI-process
boundary ends the owner of those WebKit resources instead of deleting persistent data or calling
private/test process-termination APIs.

Native acceptance must still prove the actual WebContent, GPU and Networking process exit.
A successful build or a reaped UI PID is not that proof. The source research and alternatives are
in `.planning/research/macos-webkit-lifecycle/BRIEF.md`.

## Verification

Build without opening a GUI:

```bash
bun install --frozen-lockfile
bash apps/gui-go/build.sh e2e
```

Run the E2E-only inherited-pipe check. It exits before App.New, daemon bootstrap and profile access:

```bash
target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go --ui-channel-probe
```

Do not run native interaction unless the user explicitly authorizes it or the machine is dedicated
to testing. The following driver shows windows, clicks the tray and reads native application state:

```bash
python3 apps/gui-go/e2e/ui_process_run.py --allow-session --out target/gui-go/ui-process-evidence
```

The driver uses a temporary HOME, a unique profile and no system clipboard. It checks red-close,
Cmd-W, repeated Close, Finder reopen, tray Open, tray Quit and Cmd-Q. It requires a stable
resident host, daemon and native-helper PID, a fresh UI PID on reopen, a working page-to-daemon
round trip, no app-owned WebKit helpers after closing, and an unchanged runtime preference during ordinary window closes.
`ui-process-assertions.json` contains the source/binary hashes and observations. The old
`dock_lifecycle_run.py` entry forwards to this driver. Native interaction and real cross-device
sync have not been executed as part of this change.

For Windows and Linux, build the E2E executable with the correct platform frontend and place
`gui-go[.exe]`, `uniclip[.exe]`, `uniclipd[.exe]` and the native helper when available in one directory.
On an explicitly authorized dedicated graphical host, run:

```bash
UC_GUI_GO_E2E_DEDICATED_HOST=1 python3 apps/gui-go/e2e/ui_process_portable_run.py --allow-session --binaries <directory> --out <new-evidence-directory>
```

Set the same environment variable in PowerShell before running the command on Windows. The driver
copies binaries into a new portable sandbox. It verifies three native Close/reopen cycles, real
page-to-daemon calls, an unchanged startup preference, stable resident/daemon/native-helper identities,
retained tray state and disappearance of positively attributed browser processes. It alternates
second-launch and tray Open handlers. It records a source manifest and binary hashes in
`ui-process-assertions.json`. Native OS tray clicks, keyboard interactions, installer/AppImage updates
and cross-device synchronization remain separate acceptance gates.

The in-page smoke scenario no longer tries to execute JavaScript after closing its own process.
The external lifecycle drivers own those assertions. The helper-request scenario uses an explicit
Hide to verify that helper Open restores an existing window. Hide is not used for main-window Close.

## 桌面模式验收

以下驱动要求专用测试桌面和显式允许会话交互，只使用隔离资料，不接触系统剪贴板：

```bash
UC_GUI_GO_E2E_DEDICATED_HOST=1 python3 apps/gui-go/e2e/desktop_modes_run.py --allow-session --binaries <directory> --out <new-evidence-directory>
```

macOS 使用 `--bundle <E2E.app> --cli <uniclip>` 替代 `--binaries`。驱动检查正常关闭保留宿主、轻量关闭退出宿主、显式/自动启动、重复启动、设置切换前后的 daemon PID 与 API 可用性。浏览器辅助进程和原生托盘交互还需执行对应平台的 `ui_process_run.py` / `ui_process_portable_run.py`。这些脚本和编译通过均不能代替 Windows/macOS 实机 WebView 清理与实际登录项验收。
