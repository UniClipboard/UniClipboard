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
- 配对:在一台设备上 `unic code`(或 GUI「添加设备 → 让设备加入本机」)显示 6 位配对码,在另一台设备上 `unic join <配对码>`。配对码 5 分钟有效、仅用一次,由 rendezvous 服务转存地址;对方仍需批准并核对指纹。也可以直接用完整地址:`unic addr` 得到本机地址,在另一台设备上 `unic pair <地址>`。配对是双向的,之后重启会自动重连(mesh,每台都互连)。设计见 [docs/design/decisions/0001-pairing-code.md](docs/design/decisions/0001-pairing-code.md)。
- **新设备需要确认**:任何未配对的设备,无论是自己连进来还是由已配对设备介绍来的,都先进入"待确认"(`unic requests` 或 GUI),批准后才会连接和同步。
  - 第三台设备加入:让它与任意一台配对即可。已配对的设备会互相交换各自已配对设备的名称与地址,于是其他设备里会出现"由 X 介绍"的待确认项,每台各自批准后互联,不必两两手动配对。
  - 对方也得批准你:连接要双方都批准才建立。你主动 `unic pair` 的设备视为已批准。
  - 拒绝(`unic reject`)后,介绍不会再打扰;该设备自己再来连则重新询问。
- **配置文件**:`UNIC_HOME/config.toml`(首次启动自动生成带注释的模板),保存即生效,无需重启。可设置设备名、配对码服务地址、大小上限、同步开关(整体暂停 / 文本 / 图片 / 文件)。写错时保留上一份有效配置并记录错误,用 `unic config` 查看当前生效值和错误;`unic config path` 打印文件路径。`inbox` 目录改动需重启 unicd。设计见 [docs/design/decisions/0002-config-file.md](docs/design/decisions/0002-config-file.md)。
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
| `packaging/` | 安装包资源(图标、plist 等,按需添加) |

## 开发

```sh
go build -o unic ./app/desktop/cmd/unic && go build -o unicd ./app/desktop/cmd/unicd
go test -race ./...
UNIC_HOME=/tmp/unicA ./unicd        # 用 UNIC_HOME 在同一台机器上跑第二个节点
```

国内网络下载依赖慢时:`GOPROXY=https://goproxy.cn,direct go mod download`。

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

历史记录与搜索、文件夹同步、选择性接收/设备授权确认、原生剪贴板变化通知(目前 400ms 轮询)。
