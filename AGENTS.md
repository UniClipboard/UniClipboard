# Agent Instructions

unic：跨设备剪贴板同步。Go 核心（网络层 tailcat，存储 SQLite），桌面界面 mygo，后续纳入 Android、iOS。项目概览见 [README.md](README.md)。

## 仓库结构

monorepo：平台无关的 Go 核心在根（`engine/` 是公开 SDK，`internal/` 是实现），各端应用在 `app/<平台>/`，每个应用有自己的 `README.md` 与 `AGENTS.md`。依赖单向：应用依赖核心，核心不依赖应用。布局与理由见 [0005](docs/design/decisions/0005-monorepo-layout.md)。

## 命令

| 任务 | 命令 |
|---|---|
| 测试（核心 + 桌面） | `go test -race ./...` |
| 构建桌面端 | `go build -o unic ./app/desktop/cmd/unic && go build -o unicd ./app/desktop/cmd/unicd` |
| 第二个本地节点 | `UNIC_HOME=/tmp/unicA ./unicd` |

## 文档目录

动手前先读与任务相关的设计决策；新的决策写进 `docs/design/decisions/`，按编号命名，并在下面登记。

### 设计决策（`docs/design/decisions/`）

| 编号 | 主题 | 摘要 |
|---|---|---|
| [0001](docs/design/decisions/0001-pairing-code.md) | 添加设备：6 位配对码 + 完整地址 | 复用 rendezvous `/v1/web-pairings`，5 分钟一次性；票据 kind `unic-pair`；配对码只找地址不授信，需批准并核对指纹；仅桌面端 |
| [0002](docs/design/decisions/0002-config-file.md) | 配置文件：TOML + 保存即生效 | `$UNIC_HOME/config.toml`，监听目录+防抖热加载；坏配置保留上一份有效值；`unic config` 查看；`inbox` 需重启 |
| [0003](docs/design/decisions/0003-engine-sdk.md) | 独立 Engine：统一接口与契约 | 公开 Go SDK，宿主负责剪贴板与系统集成；定义生命周期、内容所有权、事件与错误；先落地契约并迁移桌面，移动端验证后续进行 |
| [0004](docs/design/decisions/0004-clipboard-pipeline-lua.md) | 剪贴板处理流水线 + Lua 插件 | 草案；Capture→Processing→Commit→Sync→Use→End；captured/received 可干预，其余事件只读；同步为内置步骤；gopher-lua 沙箱（无 os/io）；本地历史库，标签/过期不同步；历史是否进 Engine 待定 |
| [0005](docs/design/decisions/0005-monorepo-layout.md) | 仓库布局：monorepo | 核心在根（`engine/`、`internal/`），应用在 `app/<平台>/`；依赖单向，桌面专属包放 `app/desktop/internal`；单一根模块 `github.com/uniclipboard/unic`，gomobile 引入时再拆 `bindings/` |
| [0006](docs/design/decisions/0006-macos-swiftui-client.md) | macOS 端：SwiftUI 客户端 | 草案；SwiftUI 作为 `unicd` 的客户端（unix socket，HTTP+JSON/SSE），不内嵌 Engine；`unicd` 增加无托盘模式；新增本机历史 API（事件不带正文）；历史先放 `internal/`；Developer ID 分发，不上 App Store |

### 接口契约

- [Engine 公共契约](docs/design/engine-contract.md)：Engine 接口、DTO、生命周期、信任、内容租约、发送与应用语义、事件恢复及错误码。修改核心、桌面适配或未来平台绑定前必须阅读，并同步维护契约与测试。

### 原型

- [`prototype/gui.html`](prototype/gui.html)：GUI 交互原型（浏览器直接打开）。

## 约定

- 设备地址与配对票据都是秘密：不要写进日志、事件或错误信息。
- 通过配对码进来的设备永远需要用户批准；不要为了方便绕过。
