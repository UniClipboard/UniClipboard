# 0005 · 仓库布局：monorepo，核心在根，应用在 `app/`

- 状态：已归档（2026-11-09）
- 落地：[仓库布局与依赖边界](../../architecture/overview.md)
- 日期：2026-11-07
- 关联：[0003 独立 Engine](../proposed/0003-engine-sdk.md)

> 2026-11-09 补充：[0015 无头服务与桌面集成](../proposed/0015-headless-cli-and-desktop-integration.md) 确定将通用 CLI/daemon 移至 `app/service/`，共享本机协议放在根 `internal/localapi/`，`app/desktop/` 只保留 GUI 与系统适配。该拆分待实现；下文目录描述拆分前的布局。

> 归档说明：原布局与核心/应用依赖边界已落地。移动端工程、gomobile 绑定与各端路径分流 CI 按原决策留待相应工程接入；2026-11-09 补充的无头服务拆分由 0015 单独推进，不属于本次归档的已实现范围。当前目录与缺口以落地架构页为准。

> 实现变化：设置类型与校验现由公开的 `engine.Settings` 提供；后续独立迁移工具将含平台凭据读取的实现放在根 `internal/v1migration/`，是下文“internal 只含平台无关包”的例外，不进入 Engine 依赖。

## 背景

仓库起初就是一个桌面应用，根目录的 `cmd/`、`internal/`、README、CI 都以桌面为前提。0003 引入公开的 `engine` 并计划接入 Android、iOS（以及后续的原生鸿蒙），需要一个能容纳多端、又不让各端互相污染的布局。

## 决策

```text
.
├── go.mod                  模块 github.com/uniclipboard/unic，一个模块
├── engine/                 公开 Go SDK（各端共用的入口）
├── plugin/                 规划：独立进程插件 SDK 与 RPC 类型（0004）
├── plugins/                外部插件源码与开发说明（0004）
├── internal/               核心实现，只含平台无关的包
│   ├── session/ wire/ store/ tcnet/ rendezvous/ clip/
│   └── plugins/
│       ├── builtintext/      builtin.text
│       └── tailcat/   builtin.tailcat（Transport 注入；0004）
├── app/
│   ├── desktop/            Go：cmd/unic、cmd/unicd，internal/{host,api,board,config,paths}
│   ├── android/            Kotlin / Gradle 工程
│   └── ios/                Swift / Xcode 工程
├── bindings/               将来：gomobile 等薄封装（Go）
└── docs/  prototype/
```

1. **依赖单向：** `app/*` 与 `bindings/` 依赖核心，核心不依赖任何 `app/*`。桌面专属的包放在 `app/desktop/internal`，利用 Go 的 `internal` 可见性，使 `bindings/` 与其他端在编译期就引不到它们。`engine` 的依赖里不得出现 GUI 工具包、文件监听、配置文件解析，由 `engine/boundary_test.go` 检查。
2. **一个根模块：** Android 与 iOS 工程是 Gradle 与 Xcode，不需要 `go.mod`；`go test ./...` 一次跑完核心与桌面端。模块路径 `github.com/uniclipboard/unic`（小写；GitHub 路径不区分大小写），让公开 SDK 可以被外部 `go get`。
3. **何时拆模块：** 引入 gomobile 时，它要求 `golang.org/x/mobile` 出现在 go.mod，会污染桌面依赖。届时把 `bindings/` 做成嵌套模块，根上加 `go.work` 供本地开发，嵌套模块用 `bindings/vX.Y.Z` 形式的 tag。在此之前不拆。
4. **核心不含桌面概念：** 会话层定义自己的 `Settings`；TOML 配置文件、文件监听、unix socket、托盘、系统剪贴板适配都属于桌面宿主。SDK 导出 `Settings.Validate`，配置文件解析委托给它，文件与 SDK 的校验规则只有一份。
5. **每个 `app/*` 有自己的 README 与 `AGENTS.md`**（命令、约束）；根上只放总览、文档登记和跨端约定。
6. **CI 按路径分工作流：** 核心变更触发所有端；`app/android/**` 只跑 Android，`app/ios/**` 只跑 iOS（macOS runner）。目前只有 Go 作业，移动端作业随各端工程一起加。

## 不做

- 每个平台一个仓库（契约与实现会分叉）；`pkg/`、`src/` 之类 Go 社区不用的目录；把 Gradle 或 Xcode 工程放在根上。
- 现在就为移动端建 `bindings/` 或拆模块：等第一个绑定真的要构建时再做。
