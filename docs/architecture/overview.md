---
paths:
  - go.mod
  - engine
  - internal
  - plugin
  - plugins
  - app/desktop
  - app/android
  - app/ios
  - app/mac/Package.swift
  - app/mac/Sources
  - .github/workflows
  - e2e/cli
  - tools/docscheck
verified: 9953351b7559b03fe110d91e5c86a36a1a9f0c38
---
# 仓库布局与依赖边界

unic 使用 monorepo：平台无关的 Go 核心位于根目录，各端宿主位于 `app/<平台>/`，由宿主装配核心并负责系统集成。

## 现状

- 根 `go.mod` 的模块路径为 `github.com/uniclipboard/unic`。当前只有一个 Go 模块，没有 `go.work` 或 `bindings/`；`go test -race ./...` 覆盖核心、插件和桌面端的 Go 测试。
- `engine/` 是公开 Go SDK，管理生命周期、设备信任、历史、处理流水线与同步调度；根 `internal/` 放核心实现及内置插件。`plugin/` 提供独立进程插件 SDK、RPC 和设置 Schema 工具，`plugins/` 放插件示例及开发说明。
- 根 `internal/` 另有独立迁移工具包 `v1migration/`，包含旧桌面数据目录识别和各系统凭据读取（例如 macOS Security.framework），不属于平台无关核心，也不由 Engine 引入。
- `app/desktop/` 是 Go 桌面宿主。`cmd/unic` 承载 CLI 与 GUI，`cmd/unicd` 承载 daemon，`cmd/unic-migrate` 是离线历史迁移工具。宿主装配、HTTP API、系统剪贴板、TOML 配置及文件监听、路径与 UI Kit 位于该应用的 `internal/`。当前桌面 GUI 已接入 mygo + 自定义 UI Kit，这也是含 macOS 在内的桌面界面选型。
- 配置业务校验由公开的 `engine.Settings.Validate` 提供；桌面 `config.Config.Validate` 校验宿主专属字段后调用它。Engine 不解析 TOML，也不监听配置文件。
- `app/mac/` 是独立的 Swift Package，保留 SwiftUI 界面与插件设置表单的实验代码，尚未连接 daemon；该客户端路线已取消，停止推进服务接入，现有实验画面不作为最终界面规格。`app/android/`、`app/ios/` 目前仅有各端 README 与 AGENTS.md，没有移动端工程。每个现有应用目录都有独立的 README 与 AGENTS.md。
- CI 当前只有 `.github/workflows/pr-checks.yml` 中的 Go lint、格式检查、race 测试与桌面入口构建；在 PR、主分支推送、合并队列或手动触发时运行，没有按应用路径分流，也不构建 Swift 或移动端。

## 不变量

- 应用依赖核心，根 `engine/`、`internal/`、`plugin/` 不得依赖任何 `app/*`。
- 桌面宿主专属 Go 包放在 `app/desktop/internal/`，利用 Go 的 `internal` 可见性阻止其他应用或未来绑定层直接导入；跨端能力通过公开 SDK 或协议暴露。独立旧数据迁移工具的现有例外是根 `internal/v1migration/`。
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
| `internal/v1migration/` | 独立旧数据迁移工具实现，包含平台专属凭据读取 |
| `plugin/`、`plugins/` | 插件 SDK、RPC、设置 Schema 与示例 |
| `app/desktop/cmd/`、`app/desktop/internal/` | Go 桌面命令入口与宿主实现 |
| `app/mac/` | 停止推进的 SwiftUI 实验原型与设置表单样例 |
| `app/android/`、`app/ios/` | 移动端接入约束与说明 |
| `e2e/cli/`、`tools/docscheck/` | CLI 多进程端到端验证与文档检查 |

## 规格与背景

- SDK 边界：[Engine 公共契约](../contracts/engine-contract.md)、[SDK 扩展类型契约](../contracts/sdk-extension-contract.md)。
- 宿主装配：[后端迁移契约](../contracts/backend-migration-contract.md)。
- 业务地图：[流水线与本机历史](pipeline.md)、[设备身份、信任与配对](identity-and-trust.md)。
- 应用说明：[桌面](../../app/desktop/README.md)、[macOS 预览](../../app/mac/README.md)、[Android](../../app/android/README.md)、[iOS](../../app/ios/README.md)。

## 缺口

- 桌面界面原型待定；mygo + 自定义 UI Kit 已接入的现有页面不代表最终布局与交互规格。
- Android、iOS 工程及移动绑定尚未建立；gomobile 接入时需补嵌套模块、工作区和版本标签约定（`bindings/vX.Y.Z`）。
- 各端原生 CI 尚未建立。随应用工程接入，再实现核心变更验证所有端、应用变更验证对应端的路径分流。
- 通用 CLI/daemon 尚在 `app/desktop/`。后续无头服务拆分由进行中的 0015 单独推进：计划迁入 `app/service/`，共享本机协议放根 `internal/localapi/`，桌面目录保留 GUI 与系统适配；这些目录当前尚不存在。
