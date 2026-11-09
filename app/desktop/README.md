# unic 桌面端

unic 的桌面应用(macOS / Windows / Linux),是公开 Go SDK(`engine`)的宿主。Go 实现:网络层用 [tailcat](https://github.com/tailscale/tailcat)(点对点 WireGuard 隧道,无账号、无控制服务器),界面用 [mygo](https://github.com/egoist/mygo)(原生 UI,无 webview),存储用 SQLite(纯 Go,无 cgo)。

## 架构

```
 unic cli ─┐                       ┌─ 设备 B 的 daemon
          ├─ unix socket (HTTP) ─ daemon ── tailcat 隧道 ──┤
 unic gui ─┘   像 docker 一样       │  (mesh,直连优先,DERP 兜底)
                                  └─ 设备 C 的 daemon
```

- `unicd`:常驻节点,是 mygo 的无窗口应用(剪贴板必须在主线程访问),带托盘/菜单栏图标。监听剪贴板、与所有已配对设备保持连接、应用远端变化。
- `unic gui` / `unic <cmd>`:都只是 daemon 的客户端,通过 `UNIC_HOME/unic.sock` 说 HTTP+JSON(`curl --unix-socket` 也能用)。
- 配对:在一台设备上 `unic code`(或 GUI「添加设备 → 让设备加入本机」)显示 6 位配对码,在另一台设备上 `unic join <配对码>`。配对码 5 分钟有效、仅用一次,由 rendezvous 服务转存地址;对方仍需批准并核对指纹。也可以直接用完整地址:`unic addr` 得到本机地址,在另一台设备上 `unic pair <地址>`。配对是双向的,之后重启会自动重连(mesh,每台都互连)。设计见 [docs/decisions/proposed/0001-pairing-code.md](../../docs/architecture/identity-and-trust.md)。
- **新设备需要确认**:任何未配对的设备,无论是自己连进来还是由已配对设备介绍来的,都先进入"待确认"(`unic requests` 或 GUI),批准后才会连接和同步。
  - 第三台设备加入:让它与任意一台配对即可。已配对的设备会互相交换各自已配对设备的名称与地址,于是其他设备里会出现"由 X 介绍"的待确认项,每台各自批准后互联,不必两两手动配对。
  - 对方也得批准你:连接要双方都批准才建立。你主动 `unic pair` 的设备视为已批准。
  - 拒绝(`unic reject`)后,介绍不会再打扰;该设备自己再来连则重新询问。
- **本机历史**:`unic history`(别名 `unic h`)——`list` 列表(`--type/--tag/--limit/--json`),`show [id|@N]` 输出内容到 stdout,`delete`(终端里确认一次,`-y` 跳过)、`tag`、`expire`、`action`。`@1` 是最新一条。
- **本地插件**:把 Lua 插件目录(含 `spec.toml` 和脚本)放进 `$UNIC_HOME/plugins/lua/` 即会被发现,但不会运行。`unic plugin list` 查看,`unic plugin enable <id>` / `disable <id>` 改 config.toml 的 `plugins.lua`,重启 unicd 生效。
- **配置文件**:`UNIC_HOME/config.toml`(首次启动自动生成带注释的模板),保存即生效,无需重启。可设置设备名、配对码服务地址、大小上限、同步开关(整体暂停 / 文本 / 图片 / 文件)。写错时保留上一份有效配置并记录错误,用 `unic config` 查看当前生效值和错误;`unic config path` 打印文件路径。`inbox` 目录改动需重启 unicd。设计见 [docs/decisions/proposed/0002-config-file.md](../../docs/decisions/proposed/0002-config-file.md)。
- **地址等同密码**:它内含 WireGuard 预共享密钥,持有者即可连接并写入你的剪贴板。

## 同步内容

文本、富文本(HTML)、图片(PNG)、文件(普通文件,单个上限 2 GiB)。启动时已在剪贴板里的内容不会被广播。

## 目录

桌面专属的代码都在本目录;平台无关的核心在仓库根(`engine/`、`internal/`),见[根 README](../../README.md)。

| 路径 | 作用 |
|---|---|
| `cmd/unicd` | daemon(托盘图标) |
| `cmd/unic` | cli 与 gui |
| `internal/host` | 桌面宿主:系统剪贴板适配(轮询、写入、防回环)、配置热更新、HTTP API 后端 |
| `internal/api` | 本地 socket API 与客户端 |
| `internal/board` | mygo 剪贴板适配 |
| `internal/config` | TOML 配置文件的解析与热加载 |
| `internal/paths` | 状态目录、socket、配置文件的位置 |
| `internal/uikit` | mygo 原生组件：Adwaita / macOS glass / Fluent 三套皮肤，亮暗主题与生成的设计 token |
| `packaging/` | 安装包资源(图标、plist 等,按需添加) |

## 开发

```sh
go build -o unic ./app/desktop/cmd/unic && go build -o unicd ./app/desktop/cmd/unicd
go test -race ./...
UNIC_HOME=/tmp/unicA ./unicd        # 用 UNIC_HOME 在同一台机器上跑第二个节点
```

国内网络下载依赖慢时:`GOPROXY=https://goproxy.cn,direct go mod download`。

界面默认跟随操作系统皮肤和亮暗模式。可用 `UNIC_UI_SKIN=linux|macos|windows .build/unic gui` 在本机预览其他皮肤。组件移植与近似范围见 [0013](../../docs/decisions/proposed/0013-mygo-uikit.md)。

GUI 测试可以导出三皮肤、亮暗主题的截图，包含空历史聚焦态和窄窗口：

```sh
UNIC_UI_TEST_ARTIFACTS="$PWD/.build/uikit-review" go test ./app/desktop/cmd/unic -run TestGUI -count=1
```

### 代码检查

使用 [golangci-lint](https://golangci-lint.run/docs/welcome/install/local/)，版本固定在 `.golangci-lint-version`（本地与 CI 保持一致）：

```sh
curl -sSfL https://golangci-lint.run/install.sh | sh -s -- -b ./bin "$(cat .golangci-lint-version)"
./bin/golangci-lint run ./...
./bin/golangci-lint fmt --diff             # 检查格式，非零退出表示需要格式化
./bin/golangci-lint fmt                    # 自动整理 gofmt / imports
```

`.golangci.yml` 启用错误处理、vet、静态分析、无效赋值、未使用代码、拼写和 nolint 注释检查。PR checks 在 PR、主分支推送及合并队列中执行全量 lint、格式检查、`go test -race ./...` 和两个入口的构建；Go 版本读取 `go.mod`，使用 `ubuntu-latest` Linux runner。需要强制合并门禁时，在 GitHub 分支保护中将 `Lint` 和 `Test and build` 设为必需检查。

## 第一阶段范围外

全文搜索、文件夹同步、选择性接收/设备授权确认、原生剪贴板变化通知(目前 400ms 轮询)。


## 从本机 UniClipboard v1 迁移

构建三个入口，并将它们放在同一个目录：

```sh
go build -o .build/unic ./app/desktop/cmd/unic
go build -o .build/unicd ./app/desktop/cmd/unicd
go build -o .build/unic-migrate ./app/desktop/cmd/unic-migrate
```

退出旧 UniClipboard，并停止使用目标目录的 `unicd`。先预检，再迁移：

```sh
.build/unic migrate --dry-run --json
.build/unic migrate --json
```

默认读取本机旧桌面数据目录，写入 `$UNIC_HOME` 或当前版本的默认 `unic` 目录。可用 `--source DIR --target DIR` 指定目录；新目录与已有历史目录均支持。目标被占用会失败，不会停止其他进程。完整迁移和预检就绪退出码为 0，部分迁移为 3，失败或取消为 1。`--json` 每行输出一条阶段进度或最终报告。

先尝试旧系统凭据；macOS 的钥匙串授权需用户手动允许。读不到时，在交互终端无回显输入旧明文口令。宿主可使用 `--passphrase-stdin` 通过标准输入传递口令；不要将口令放进命令行或环境变量。命名 profile 可用 `--service UniClipboard-PROFILE` 指定旧 service。

成功项逐条持久化，中断后用相同命令续跑。工具会复制文件内容、保留标签与旧时间，并在关闭和重开目标后校验内容摘要。文件夹保存为 ZIP；原文件已丢失、密文损坏或超出资源限制时明确报告 `partial`，不会宣称全部完成。已迁移条目不会因重跑而重复；已主动删除的目标条目不会被复活。

迁移后启动 `.build/unicd`，打开 `.build/unic gui`，默认「历史」页可浏览迁移记录、按标签筛选和使用内容。此处指 Go 桌面应用；`app/mac` 的独立 SwiftUI 客户端未接入本次功能。

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
unic config set file_cache.retention_hours 24
unic config set file_cache.auto_cleanup true
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
