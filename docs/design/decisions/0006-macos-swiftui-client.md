# 0006 · macOS 端：SwiftUI 原生界面，作为 daemon 的客户端

- 状态：草案（未实现）
- 日期：2026-11-07
- 关联：[0003 Engine SDK](0003-engine-sdk.md)、[0004 流水线与历史](0004-clipboard-pipeline-lua.md)、[0005 仓库布局](0005-monorepo-layout.md)
- 原型：待提供（Mac 端界面设计另行给出）

## 背景

桌面界面现在是 mygo 的原生 UI（自绘，不是系统控件）。macOS 上希望得到 SwiftUI 的原生外观与行为：系统控件、`MenuBarExtra`、Liquid Glass（`.glassEffect()`）、辅助功能与系统更新带来的变化。Windows、Linux 继续用 mygo。

同时，Mac 端需要**剪贴板历史**，而目前后端没有历史：Engine 只有 `Received` 列表（仅收到的内容），daemon API 没有历史接口，0004 把“历史放哪”列为待定。

## 决策

### 1. SwiftUI 应用是 `unicd` 的客户端，不内嵌 Engine

```text
SwiftUI app (app/macos/) ── unix socket, HTTP+JSON/SSE ──> unicd（无托盘）── tailcat ──> 其他设备
```

- 沿用现有“GUI 只是 daemon 的客户端”的结构（`$UNIC_HOME/unic.sock`，`/v1/*`，SSE 事件）。系统剪贴板监听、写入、防回环仍在 `unicd`，Swift 侧不重复实现。
- **不做**：用 gomobile 把 Engine 打成 xcframework 内嵌进 Mac 应用。理由：移动端绑定尚未验证（0003），会提前引入 `bindings/` 嵌套模块与 `go.work`（0005 §3），且需要把 context/channel/lease 映射为 Swift 句柄。等 iOS 绑定真的要构建时再评估，届时 Mac 可以迁移；本决策不阻碍它，因为界面只依赖下面的 API 契约。

### 2. 进程与生命周期

- `unicd` 增加**无托盘模式**（配置或启动参数），Mac 应用提供菜单栏图标；两者不同时显示。Windows、Linux 行为不变。
- Mac 应用把 `unicd` 作为**登录项**随包分发，用 `SMAppService`（macOS 13+）注册与启动；应用只在 socket 不可达时提示，不直接拉起子进程管理其生命周期。
- 应用退出不停止同步；用户在“设置”里选择是否随登录启动、是否退出时一并停止 daemon。
- 状态目录、socket、数据库位置不变（`internal/paths`）。socket 路径受 104 字节限制，沿用现有约定。

### 3. 分发

- Developer ID 签名 + 公证，在 App Store 之外分发。
- **不上 Mac App Store**：沙盒不允许读取 `~/Library/Application Support/unic` 下的 socket，也不允许常驻的内嵌守护进程。若以后要上架，须另写决策，改用方案 B（内嵌 Engine）。

### 4. 历史 API（daemon 侧新增）

历史是本机数据，不进 wire。接口形状，具体字段在实现时写入契约：

| 路由 | 作用 |
|---|---|
| `GET /v1/history?q=&kind=&pinned=&before=&limit=` | 列表，按时间倒序，固定项靠前；`q` 匹配正文、标签、来源设备 |
| `GET /v1/history/{id}` | 单条元数据与文本类预览 |
| `GET /v1/history/{id}/content` | 二进制内容（图片、文件、完整文本） |
| `POST /v1/history/{id}/use` | 设为当前剪贴板（等价于 `PUT /v1/clipboard` 以历史项为来源；保证与现有防回环一致） |
| `PATCH /v1/history/{id}` | 固定、标签（仅本机） |
| `DELETE /v1/history/{id}`、`DELETE /v1/history?keep_pinned=1` | 删除、清空 |

- **事件不带正文**（沿用 CLAUDE.md 约定）：新增 `history-added`、`history-updated`、`history-removed`，只带 `id`，客户端再拉取。
- 访问边界是 socket：任何能连 socket 的本机进程都能读历史正文，目录权限（0700）是唯一边界。
- 数据模型、保留上限、过期清理沿用 0004 §7、§8；标签与过期不同步。
- “使用”会让条目移到最前并标为当前；该时间戳是本机的 `updated_at`，不改 `created_at`。

### 5. 历史放哪（承接 0004 的待定项）

按 0004 的倾向：流水线与历史先以 **`internal/`** 落地（`history.Store`、`pipeline`），不进入公开的 `engine` 签名；daemon 通过 `internal/api` 暴露第 4 节的 HTTP 接口。

- 好处：Mac 界面可以先做；iOS 到来时若历史进入 Engine，只是把同一套 `internal` 接口公开，HTTP 接口形状不变。
- 代价：在公开前，iOS/Android 无法复用；需要在评估 0003 后再决定哪些方法公开。
- 本决策**不**替 0004 做最终决定，只要求第 4 节接口与存储不依赖桌面专属包。

### 6. 目录与工程

```text
app/macos/
├── README.md、AGENTS.md
├── Unic.xcodeproj（或 Package.swift + xcodeproj）
├── Sources/
│   ├── UnicKit/       socket 客户端：HTTP over unix socket、SSE、Codable 模型
│   ├── UnicApp/       主窗口（NavigationSplitView）、设置
│   ├── MenuBar/       MenuBarExtra
│   └── QuickPanel/    NSPanel 承载的 SwiftUI、全局热键、粘贴
└── Tests/
```

- 依赖单向：`app/macos` 依赖 daemon 的 HTTP 契约，不依赖 Go 源码。CI 按路径触发，仅 `app/macos/**` 变更时在 macOS runner 上构建与测试（0005 §6）。
- **Swift 客户端**：`URLSession` 不支持 unix socket，使用 Network.framework 的 `NWConnection` 或 swift-nio / AsyncHTTPClient（需实测选定，二者都不应引入进入 Go 侧的依赖）。SSE 自行解析。
- **接口契约单一来源**：给 `/v1` 写一份 OpenAPI（放 `docs/design/`），Go 侧测试与 Swift 模型都对着它；Swift 模型可手写，也可用 swift-openapi-generator，实现时再定。在此之前，以 `internal/api/api.go` 为准。

### 7. Mac 端专属能力（全部在 Swift 侧）

- **快速面板**：全局热键（默认 ⌥⌘V，可改），非激活式 `NSPanel`；⏎ 粘贴到前台应用、⇧⏎ 仅复制、⌘1–9 直达、⌘P 固定。
- **粘贴到前台应用**需要合成 ⌘V，依赖**辅助功能**授权。未授权时降级为“仅复制并提示”，不静默失败；首次使用时解释用途后再引导授权。
- **玻璃**：菜单栏弹窗与快速面板用系统玻璃材质（macOS 26+）；更低版本回退为 `NSVisualEffectView` 材质。最低系统版本在建工程时确定，倾向 macOS 14（`MenuBarExtra` 与 `SMAppService` 的实用下限为 13）。
- **配对**沿用 0001：6 位配对码与完整地址，通过现有 API；通过配对码进来的设备永远需要用户批准，Swift 侧不得提供绕过。
- **秘密不落日志**：设备地址、配对票据、剪贴板正文不写入 `os_log`、崩溃报告或事件；Swift 日志与 Go 侧同一约定。

## 不做

- 在 Swift 里重写同步、配对或剪贴板监听逻辑。
- 现在建 `bindings/` 或引入 gomobile。
- 让 Swift 应用直接读 SQLite（绕过 daemon 会与并发写、过期清理、迁移冲突）。
- 为 Mac 端单独复制一套历史存储。

## 待定

- socket 客户端选 `NWConnection` 还是 AsyncHTTPClient（实测稳定性、SSE 长连接、断线重连）。
- 历史的保留上限（条数、字节、天数）与图片/文件的存储策略，沿用 0004 §7 的“超限淘汰最旧、未固定者”，具体数值待定。
- 最低 macOS 版本。
- “退出应用时是否停止 daemon”的默认值。

## 验收

- `unicd` 无托盘模式下同步行为与现在一致；`go test -race ./...` 通过。
- 历史 API 有测试：分页、搜索、固定、删除、过期、事件不含正文。
- Mac 应用在 daemon 未运行、重启、socket 断开后能重连，界面状态与提供的原型一致。
- 辅助功能未授权时快速面板降级为仅复制并给出提示。
