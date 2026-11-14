# Desktop application

`unic-desktop` owns the mygo GUI, native clipboard adapter, tray and notifications.
`unicd` runs separately and owns Engine, history, configuration and networking.
The UI-free `unic` CLI and `unicd` live in [the service application](../service/README.md).
See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).

```text
unic -----------+-- local socket -- unicd -- Engine / network / history
unic-desktop --+
     +-- mygo / clipboard / tray / notifications
```

| Path | Responsibility |
|---|---|
| `cmd/unic-desktop` | GUI and graphical session lifecycle |
| `internal/board` | Native clipboard access |
| `internal/uikit` | Adwaita, macOS glass and Fluent component skins |
| `internal/desktopui` | Shared desktop renderer and production service binding |
| `internal/i18n` | Embedded translation catalogue and lookup |
| `internal/desktopdemo` | Independent preview launcher and synthetic fixtures |

Closing the window hides it and keeps the adapter active. The tray or a second launch shows it again.
Explicit Quit stops the desktop process and its owned service. Lightweight mode runs only the standalone service.
Pairing requires explicit mutual approval with verified fingerprints. See [identity and trust](../../docs/architecture/identity-and-trust.md).

## 开发

Run the development desktop from the repository root:

```sh
make run-desktop
```

The command rebuilds `unic`, `unicd` and `unic-desktop` in `dist/`, then starts normal desktop mode.
It stores development data in `.cache/desktop-dev` and clears any inherited `UNIC_SOCK` override.
The development profile persists across runs and stays separate from the default profile.
Run the command again after changing code.

### Native layout demo

Build `unic-desktop-demo` with `go build -o .build/unic-desktop-demo ./app/desktop/cmd/unic-desktop-demo`, then run it to open the restored layouts with local fixtures.
Use `UNIC_UI_SKIN=linux|macos|windows` to select a skin.
Use `--theme light|dark|system` to select a color mode.
Use `--page history|devices|settings|quick-panel` to choose the initial page.
The demo does not require `unicd`. All changes remain in memory.
The normal `unic-desktop` entry uses the same history, device sheet and settings composition with real service data. Fresh profiles show first-install setup and optional offline v1 migration. Completed setup and established v2 profiles open history directly. Settings > Storage > Migrate from v1 permits later import. See [startup and migration behavior](../../docs/architecture/overview.md#live-desktop-composition).
See the [live-data execution receipt](../../docs/exec/desktop-live-data.md) for supported operations, remaining gaps and nonvisual process acceptance.
See [UI-DEMO.md](UI-DEMO.md) for the reference commit, scope, and repeatable acceptance steps.

```sh
go build -o unic-desktop ./app/desktop/cmd/unic-desktop
go build -o unic ./app/service/cmd/unic
go build -o unicd ./app/service/cmd/unicd
go test -race ./...
UNIC_HOME=/tmp/unicA ./unicd        # 用 UNIC_HOME 在同一台机器上跑第二个节点
```

国内网络下载依赖慢时:`GOPROXY=https://goproxy.cn,direct go mod download`。

界面默认跟随操作系统皮肤和亮暗模式。可用 `UNIC_UI_SKIN=linux|macos|windows .build/unic-desktop` 在本机预览其他皮肤。组件移植与近似范围见 [0013](../../docs/decisions/proposed/0013-mygo-uikit.md)。

GUI 测试可以导出三皮肤、亮暗主题的截图，包含空历史聚焦态和窄窗口：

```sh
make desktop-e2e CASE=gui-navigation
make desktop-e2e CASE=gui-approval-reconnect
make desktop-e2e CASE=gui-empty-layout
```

### 代码检查

使用 [golangci-lint](https://golangci-lint.run/docs/welcome/install/local/)，版本固定在 `.golangci-lint-version`（本地与 CI 保持一致）：

```sh
curl -sSfL https://golangci-lint.run/install.sh | sh -s -- -b ./bin "$(cat .golangci-lint-version)"
./bin/golangci-lint run ./...
./bin/golangci-lint fmt --diff             # 检查格式，非零退出表示需要格式化
./bin/golangci-lint fmt                    # 自动整理 gofmt / imports
```

`.golangci.yml` 启用错误处理、vet、静态分析、无效赋值、未使用代码、拼写和 nolint 注释检查。PR checks 在 PR、主分支推送及合并队列中执行全量 lint、格式检查、`go test -race ./...` 和三个入口的构建；Go 版本读取 `go.mod`，使用 `ubuntu-latest` Linux runner。需要强制合并门禁时，在 GitHub 分支保护中将 `Lint` 和 `Test and build` 设为必需检查。

## 第一阶段范围外

Full-text search, fine-grained device policies and native clipboard change notifications remain outside the live desktop binding. One-shot directory transfer is implemented under the [directory contract](../../docs/contracts/directory-contract.md); automatic directory sync defaults off. Device pairing already requires explicit approval.


## 从本机 UniClipboard v1 迁移

构建两个入口（迁移是 `unic migrate` 子命令，没有独立程序）：

```sh
go build -o .build/unic ./app/service/cmd/unic
go build -o .build/unicd ./app/service/cmd/unicd
```

退出旧 UniClipboard，并停止使用目标目录的 `unicd`。先预检，再迁移：

```sh
.build/unic migrate --dry-run --json
.build/unic migrate --json
```

默认读取本机旧桌面数据目录，写入 `$UNIC_HOME` 或当前版本的默认 `unic` 目录。可用 `--source DIR --target DIR` 指定目录；新目录与已有历史目录均支持。目标被占用会失败，不会停止其他进程。完整迁移和预检就绪退出码为 0，部分迁移为 3，失败或取消为 1。`--json` 每行输出一条阶段进度或最终报告。

先尝试旧系统凭据；macOS 的钥匙串授权需用户手动允许。读不到时，在交互终端无回显输入旧明文口令。宿主可使用 `--passphrase-stdin` 通过标准输入传递口令；不要将口令放进命令行或环境变量。命名 profile 可用 `--service UniClipboard-PROFILE` 指定旧 service。

成功项逐条持久化，中断后用相同命令续跑。工具会复制文件内容、保留标签与旧时间，并在关闭和重开目标后校验内容摘要。文件夹保存为 ZIP；原文件已丢失、密文损坏或超出资源限制时明确报告 `partial`，不会宣称全部完成。已迁移条目不会因重跑而重复；已主动删除的目标条目不会被复活。

迁移后启动 `.build/unicd`，打开 `.build/unic-desktop`，默认「历史」页可浏览迁移记录、按标签筛选和使用内容。此处指 Go 桌面应用；`app/mac` 的独立 SwiftUI 客户端未接入本次功能。

`unic` 随常规发布流程构建，迁移不另行打包。跨平台编译不等同于全部平台的钥匙串运行验收。格式范围、幂等、容量策略和验收记录见[迁移设计](../../docs/decisions/proposed/0011-v1-history-migration.md)和[执行记录](../../docs/exec/0011-v1-history-migration.md)。

配对审批必须显式提供另一台设备上显示的完整指纹：`unic device approve <id> '<fingerprint>'`。HTTP `POST /v1/pending/{id}/approve` 必须带 `fingerprint`、`revision` 与 `bindings:[{id,revision}]`（修订来自 `/v1/pending`）；设备或待批准关联变化时整个审批失败，刷新后重新核对。普通 `/v1/status` 不含票据；`unic device addr` 与 GUI「生成票据」显式调用 `POST /v1/pairing-ticket`，返回的秘密票据 5 分钟有效且仅用一次。详见[身份与信任](../../docs/architecture/identity-and-trust.md)。

## CLI sync and file settings

Read the saved settings, change one TOML value, or apply a complete file:

```sh
unic config --json
unic config set sync.automatic false
unic config set sync.on_restore true
unic config set sync.files true
unic config set limits.max_file_bytes '"100MB"'
unic config set file_cache.max_bytes '"500MB"'
unic config set history.auto_cleanup true
unic config set history.retention_days 180
unic config set history.max_items 0
unic config set history.excluded_tags '["keep", "work"]'
unic config apply /absolute/path/config.toml
unic history use @1 --request-id restore-001 --json
unic dev cache --json
```

Saving `config.toml` also reloads valid settings. Invalid edits keep the last valid values.
Change `inbox` to an existing absolute custom directory and restart the daemon to use that location.
Use `unic config set inbox '"/absolute/path/received"'` to save the custom directory.
Use `unic config set inbox '""'` to return to the default cache after restart.

Quota, retention, and cleanup cover only the default managed cache.
Custom received files are user-managed. History deletion, lease release, expiry, and restart never delete these files.
See [desktop runtime](../../docs/architecture/desktop-runtime.md) for limits, save conflicts, and file ownership.

Custom receive directories and file creation belong to the desktop host.
Engine receives a file stream callback and persists an opaque host reference.
The desktop host supplies a separate private cache directory for Engine quota and cleanup.

Unified automatic cleanup removes eligible history and its managed files together.
The Settings tab offers age, count, excluded tags, and the per-device cache quota.
A matching excluded tag preserves both history and managed files. Custom received files remain user-managed.
When cleanup is off or cannot free sufficient cache space, new file reception fails rather than bypassing the policy.
See [desktop runtime](../../docs/architecture/desktop-runtime.md).

## Independent desktop program

Build `go build -o dist/unic-desktop ./app/desktop/cmd/unic-desktop`.
Run `unic-desktop` to restart the profile service and attach the GUI. Use `unic-desktop-demo` for the existing isolated UI fixture.
The desktop program contains the GUI, system clipboard adapter, tray and notifications.
Closing its window keeps the adapter running. Explicit quit stops desktop integration and the owned service.
See [headless architecture](../../docs/architecture/headless.md).

## Startup and runtime modes

Normal mode starts a fresh sibling `unicd` and owns its lifetime.
If the profile already has a service, the launcher stops that generation before starting its own.
Desktop process exit also stops its service. Window close only hides the window.
Lightweight mode starts or reuses a standalone service, then exits the launcher without loading the GUI runtime.

```toml
[desktop]
mode = "normal" # "normal" or "lightweight"
launch_at_login = false
start_in_background = false
```

The Settings tab edits these values through the shared configuration API.
`start_in_background` applies to every normal launch, including manual launches.
The initial window stays hidden; tray Open or a second normal launch shows it.
Mode and background changes apply on the next launch.
Use `unic-desktop --mode normal` to reopen settings when the saved mode is lightweight.
Use `unic-desktop --mode lightweight` for an explicit service-only launch.
Neither override changes the saved mode.

Login registration uses the existing mygo native API.
A running normal desktop applies changed intent and shows actual OS status and failures.
External system disable remains effective until the user retries or changes the requested value.
macOS requires a packaged `.app`; flat binaries cannot register a login item.
The native API does not preserve custom profile environment overrides, so enabling login startup with `UNIC_HOME` or `UNIC_SOCK` set fails visibly.
A setting changed while only the service is running waits for the next normal desktop session.
See [startup architecture](../../docs/architecture/headless.md#desktop-startup-settings).

Repeatable process acceptance without opening a GUI:

```sh
make desktop-e2e CASE=startup-lifecycle
```

The manifest records source and executable SHA-256 values, process checks and native acceptance gaps.
See the [acceptance receipt](../../docs/exec/desktop-startup-lifecycle.md).

## Desktop end-to-end acceptance

```sh
make desktop-e2e
```

This is the canonical entry for Desktop acceptance. It owns the environment, source snapshot, binaries, isolated profiles and result paths. The command prints one HTML report containing case results, assertions, native-window screenshots and continuous recordings. Use `make desktop-e2e-list` to list cases or `make desktop-e2e CASE=history-search-copy` to select one.

See the [test guide](e2e/README.md) for requirements, evidence boundaries and adding a case. The [generated catalogue](e2e/CASES.md) lists what actually runs. Native-window content evidence does not establish system clipboard, OS focus or cross-application paste.

## Search

The main history window and quick panel search all persisted history through the service.
Text, HTML fallback, file basenames, extensions, tags, source and calendar filters use the [Engine search contract](../../docs/contracts/history-search-contract.md).
The main editor matches all parsed words and all selected tags. The quick panel matches the literal phrase and any selected tag.
Open the real-data quick panel from Settings or the history menu. Its Copy/Delete actions use the service. OS paste and global activation preferences remain separate.

Search process acceptance runs as `search-contract` under `make desktop-e2e`. See the [test guide](e2e/README.md).

On other platforms, use an existing canonical temporary directory instead of `/private/tmp`.
This process acceptance does not start a native app, offscreen renderer or system clipboard. See the [receipt](../../docs/exec/desktop-search-implementation.md).

### Quick-panel system commands

The production host registers Command+Ctrl+V on macOS and Ctrl+Alt+V on Windows/Linux by default. Change or disable it in Settings > Quick Panel. A conflicting replacement preserves the previous working registration and offers retry. The tray also opens the panel. Lightweight mode has no graphical shortcut host.

Open the panel from another application to capture its target. Enter applies history and sends paste to that target. Shift+Enter applies the existing plain-text Action first. Command/Ctrl+Enter sends paste and reopens the panel. Command/Ctrl+C copies without input injection. macOS requires Accessibility permission. Wayland keeps Copy but cannot automatically restore arbitrary target focus or inject paste.

The [system contract](../../docs/contracts/desktop-quick-panel-contract.md) defines cancellation, clipboard ownership and platform limits. The [execution receipt](../../docs/exec/desktop-quick-panel-system.md) distinguishes process evidence from native acceptance.

## Interface languages

Open Settings > General > Interface language to select Simplified Chinese, English, Japanese, Brazilian Portuguese or Russian. The selection saves to `desktop.language` and applies without restarting. Simplified Chinese is the default. The demo keeps language changes in memory. Clipboard content, device names, filenames and user tags remain unchanged.

```toml
[desktop]
language = "pt-BR"
```

Use `make desktop-e2e CASE=i18n` for the repeatable software-view language matrix. See [desktop languages](../../docs/architecture/overview.md#desktop-languages) and the [acceptance receipt](../../docs/exec/desktop-i18n.md).

Built-in Text, Images, Files and Links tags use the selected interface language. The default `builtin.classify` processor annotates new captured, received and Action content. Links also carry the Text tag. Existing history is not backfilled. Built-in tags cannot be renamed, merged or deleted in Desktop. The inline ComboField lists unattached custom tags and offers a five-color creation row. Custom chip removal detaches the tag from that entry. Colors persist through the existing desktop configuration API. Sidebar visibility is retained during refresh within the current session. See [classification](../../docs/architecture/pipeline.md#built-in-content-tags) and [tag controls](../../docs/architecture/overview.md#tag-controls-and-colors).
