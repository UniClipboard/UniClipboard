# Agent Instructions

unic：跨设备剪贴板同步。Go，网络层 tailcat，界面 mygo，存储 SQLite。项目概览见 [README.md](README.md)。

## 命令

| 任务 | 命令 |
|---|---|
| 构建 | `go build -o unic ./cmd/unic && go build -o unicd ./cmd/unicd` |
| 测试 | `go test -race ./...` |
| 第二个本地节点 | `UNIC_HOME=/tmp/unicA ./unicd` |

## 文档目录

动手前先读与任务相关的设计决策；新的决策写进 `docs/design/decisions/`，按编号命名，并在下面登记。

### 设计决策（`docs/design/decisions/`）

| 编号 | 主题 | 摘要 |
|---|---|---|
| [0001](docs/design/decisions/0001-pairing-code.md) | 添加设备：6 位配对码 + 完整地址 | 复用 rendezvous `/v1/web-pairings`，5 分钟一次性；票据 kind `unic-pair`；配对码只找地址不授信，需批准并核对指纹；仅桌面端 |

### 原型

- [`prototype/gui.html`](prototype/gui.html)：GUI 交互原型（浏览器直接打开）。

## 约定

- 设备地址与配对票据都是秘密：不要写进日志、事件或错误信息。
- 通过配对码进来的设备永远需要用户批准；不要为了方便绕过。
