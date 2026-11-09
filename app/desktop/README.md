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

Closing the window hides it and keeps the adapter active. The tray or a second launch shows it again.
Explicit Quit stops the desktop process. The service continues receiving and storing content.
Pairing requires explicit mutual approval with verified fingerprints. See [identity and trust](../../docs/architecture/identity-and-trust.md).

## 开发

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
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/uikit-review" go test ./app/desktop/cmd/unic-desktop -run TestGUI -count=1
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

全文搜索、文件夹同步、选择性接收/设备授权确认、原生剪贴板变化通知(目前 400ms 轮询)。


## 从本机 UniClipboard v1 迁移

构建三个入口，并将它们放在同一个目录：

```sh
go build -o .build/unic ./app/service/cmd/unic
go build -o .build/unicd ./app/service/cmd/unicd
go build -o .build/unic-migrate ./app/service/cmd/unic-migrate
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

六平台 CLI 构建：

```sh
app/desktop/scripts/build-migration.sh
```

产物位于 `dist/migration/`，包含 macOS / Windows / Linux × arm64 / amd64 和 `SHA256SUMS`。跨平台编译不等同于全部平台的钥匙串运行验收。格式范围、幂等、容量策略和验收记录见[迁移设计](../../docs/decisions/proposed/0011-v1-history-migration.md)和[执行记录](../../docs/exec/0011-v1-history-migration.md)。

配对审批必须显式提供另一台设备上显示的完整指纹：`unic approve <id> '<fingerprint>'`。HTTP `POST /v1/pending/{id}/approve` 必须带 `fingerprint`、`revision` 与 `bindings:[{id,revision}]`（修订来自 `/v1/pending`）；设备或待批准关联变化时整个审批失败，刷新后重新核对。普通 `/v1/status` 不含票据；`unic addr` 与 GUI「生成票据」显式调用 `POST /v1/pairing-ticket`，返回的秘密票据 5 分钟有效且仅用一次。详见[身份与信任](../../docs/architecture/identity-and-trust.md)。

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
unic cache --json
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
Start `unicd` separately, then run `unic-desktop`.
The desktop program contains the GUI, system clipboard adapter, tray and notifications.
Closing its window keeps the adapter running. Explicit quit stops desktop integration.
See [headless architecture](../../docs/architecture/headless.md).
