# 0006 · macOS 端：SwiftUI 原生界面，作为 daemon 的客户端

- 状态：草案（未实现）
- 日期：2026-11-07
- 关联：[0003 Engine SDK](0003-engine-sdk.md)、[0004 流水线与历史](0004-clipboard-pipeline-lua.md)、[0005 仓库布局](0005-monorepo-layout.md)
- 原型：待提供（Mac 端界面设计另行给出）

## 背景

桌面界面现在是 mygo 的原生 UI（自绘，不是系统控件）。macOS 上希望得到 SwiftUI 的原生外观与行为：系统控件、`MenuBarExtra`、Liquid Glass（`.glassEffect()`）、辅助功能与系统更新带来的变化。Windows、Linux 继续用 mygo。

同时，Mac 端需要**剪贴板历史**，而目前后端没有历史：Engine 只有 `Received` 列表（仅收到的内容），daemon API 没有历史接口，0004 已确定历史与流水线由 Engine 管理，业务实现放在 `engine/`，尚未实现。

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

历史是本机数据，不进 wire。**2026-11-07 实现状态**：下列路由已实现并有测试（`app/desktop/internal/api`、`host/history.go`），形状按 Engine 实际能力收敛；固定（pin）、正文搜索、清空历史不在 0004 首版范围，未实现，列在“未实现”。

| 路由 | 作用 |
|---|---|
| `GET /v1/history?limit=&cursor=&kind=&origin=&tag=&device=&from=&before=` | 列表，最新在前，keyset 游标分页；`kind`/`origin`/`tag` 可重复，字段间 AND，`tag` 要求全部匹配；响应 `{items, next_cursor, revision}`，`items` 永不为 null |
| `GET /v1/history/{id}` | 单条元数据：kind、来源（含设备名）、时间、过期、标签、各部件的种类/MIME/大小、`sync_state` 与按后端的 `backends[{backend_id,state,disposition,error_code}]`（pending 在 daemon 重启后变 interrupted，不代表对端已收到）；不含正文、文件名、路径 |
| `GET /v1/history/{id}/content?part=N` | 第 N 个部件（默认 0）的原始字节；文件名只在 `Content-Disposition` |
| `POST /v1/history/{id}/use` | 写入系统剪贴板，走与接收内容相同的观察/应用协议（不重复入历史、不同步、不当作新复制）；文件按原路径放入剪贴板，文件已不在则失败 |
| `GET /v1/actions` | 插件声明的 Action 列表：`{actions:[{key, plugin_id, id, title, accepts}]}`，按 key 排序；**待实现**，契约见[Action 契约](../clipboard-action-contract.md) |
| `POST /v1/history/{id}/actions/{key...}` | 用户主动对该条运行 Action：Engine 生成 origin=`action` 的新历史条目，daemon 随即经应用协议把它写入系统剪贴板（用户触发即意图，不是自动写回），返回 `{item}`（新条目，含 `plugin_id`）；来源条目不变，失败不留条目；写剪贴板失败时条目仍在历史，返回 502 并带 `item`；无参数；已实现 |
| `PATCH /v1/history/{id}` | body `{revision, tags?, expires_at_ms?}`；`revision` 必填，不符返回 409 |
| `DELETE /v1/history/{id}?revision=N` | 删除，`revision` 必填；已取得内容的读取者可读完 |

- **事件不带正文**（沿用 CLAUDE.md 约定）：新增单一事件 `{"kind":"history","info":"<id>"}`，覆盖新增/更新/删除/到期/淘汰；客户端重新 `GET`，404 表示已移除。Engine 事件不区分增改删，原计划的 `history-added/updated/removed` 三种因此合并。
- 访问边界是 socket：任何能连 socket 的本机进程都能读历史正文，目录权限（0700）是唯一边界。
- 数据模型、保留上限、过期清理沿用 0004 §7、§8；标签与过期不同步。**复制文件时历史只记路径，不拷贝**（见历史契约 §6）：文件被移动或修改后，条目元数据保留，内容读取与 `use` 返回错误。
- 默认保留 10000 条 / 4GiB（仅统计历史自己存储的字节），尚未映射到 `config.toml`。
- GUI 在历史条目的操作菜单里列出 `accepts` 包含该条类型的 Action（首版只有文本）。
- `use` 不改 `created_at`，条目也不会因此移到最前（0004 没有“最近使用”排序）。

**未实现**：固定项（pin）、`q` 正文/标签/来源搜索、`DELETE /v1/history`（清空）、`keep_pinned`；若 SwiftUI 客户端需要，另议是否扩展 Engine。

### 5. 历史归 Engine 管理（承接 0004）

历史与流水线的业务实现都放在 `engine/`，由公开 Engine API 提供。daemon 通过桌面 HTTP 适配暴露第 4 节接口，不另建历史存储或流水线；未来 iOS/Android 绑定复用同一套 Engine 能力。具体 SDK 签名与持久资源契约在实现前补齐，不依赖桌面专属包。

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
