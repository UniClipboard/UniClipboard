# 0004 · 事件驱动的剪贴板处理流水线 + Lua 插件

- 状态：草案（待评估项见“待定”；未实现）
- 日期：2026-11-07
- 关联：[0002 配置](0002-config-file.md)、[0003 Engine SDK](0003-engine-sdk.md)
- 参考：架构图（Capture → Processing → Commit → Sync → Use → End，事件总线，Lua 插件 API）

## 背景与目标

现在本机复制后，桌面宿主轮询到变化（`app/desktop/internal/host/clipboard.go` 的 `tick`）就交给 `Engine.ObserveClipboard`（`engine/clipboard.go`）直接同步，中间没有可介入的位置；接收远端内容同理（`applyLoop` → `Engine.BeginApply`）。

目标：用户复制后，内容先经过一条**处理流水线**，Lua 插件可在其中修改内容、加标签、控制是否同步、设置过期时间；同步本身只是流水线里的**内置步骤**，不再是写死的出口。

## 决策

### 1. 生命周期

```text
Capture/Receive → Processing → Commit → Sync → Use → End
   (捕获/接收)     (自动化)    (入历史)  (同步)  (使用)  (删除/过期)
```

| 阶段 | 事件 | 类型 | 说明 |
|---|---|---|---|
| Capture | `clipboard.captured`（本机）、`clipboard.received`（远端） | **可干预** | 同步串行调用插件，可改内容与策略 |
| Commit | `clipboard.committed` | 通知 | 已写入历史；此后内容与策略不可被插件改动（只能经 `updated`） |
| Sync | `clipboard.sync.requested/started/completed/failed` | 通知 | 由内置同步步骤发出 |
| Receive | `clipboard.receive.started/completed/failed` | 通知 | 远端内容的接收与应用 |
| Use | `clipboard.pasted` | 通知 | 用户经 UI 使用；系统直接粘贴无法观察，不承诺 |
| End | `clipboard.deleted`、`clipboard.expired` | 通知 | 用户删除或到期 |
| 其他 | `clipboard.updated`、`device.connected/disconnected`、`app.started/shutdown` | 通知 | |

“可干预”只有 captured/received 两个，其余事件只读。这样同步与存储始终基于一份确定的最终结果。

### 2. 流水线与同步

- 流水线是 Engine 内的有序阶段链；每条内容在阶段间传递一个 `Item` 和一个 `Policy`：
  - `Policy{ persist bool, sync bool, expires_at *time, tags []string }`
  - 默认 `persist=true, sync=按 0002 的 sync.* 配置`。
- **同步是内置扩展**，订阅 committed，读取 `Policy.sync`；0002 的 `sync.enabled/text/images/files` 仍是它的前置过滤，插件的 `set_sync(true)` 不能越过用户在配置里关掉的类型。
- `persist=false` 的内容不入历史，但仍可按 `sync` 同步。
- 远端内容走 received 阶段，插件同样可以改内容、标签、过期；接收方的 `set_sync` 只影响本机是否**再转发**，首版不转发（现有语义），所以对 received 实际只用 `persist/tags/expires`。

### 3. 防回环

插件改内容意味着需要把结果写回系统剪贴板，这会再次触发捕获。沿用 0003 的 apply token / 基线 hash 思路，并增加：

- 每个 `Item` 带 `origin`（`LocalSystem | RemoteDevice(id) | Plugin(name) | …`）和 `automation_depth`。
- 写回产生的新捕获继承 `origin=Plugin`，`automation_depth+1`；深度超过上限（默认 3）直接 commit，不再进插件。
- 写回后的基线 hash 记为已知，不重复触发 captured。

### 4. Lua 运行时与沙箱

- 使用 `gopher-lua`（纯 Go，无 cgo，不妨碍后续移动端）。
- **只加载** base（去掉 `dofile/loadfile/load`）、`string`、`table`、`math`、`utf8` 风格工具；**不加载** `os`、`io`、`debug`、`package`/`require`。
- 每次回调有墙钟超时（默认 200ms）与指令数上限；超限终止该次调用。内存上限用 gopher-lua 的注册表/调用栈大小限制加正文大小限制近似，不承诺精确计量。
- 每个插件独立的 Lua 状态；回调串行执行，不共享全局表。
- 插件错误：丢弃该插件本次对 Item/Policy 的改动，继续后面的插件，不阻断流水线。**注意**：这意味着“敏感信息检测”类插件出错时内容会照常同步。提供 `plugins.on_error = "continue" | "block_sync"`，默认 `continue`，写入配置模板注释说明风险。
- 日志与事件**不含剪贴板正文**，只含插件名、事件名、错误码；Lua 错误信息在记录前脱敏（可能夹带正文）。

### 5. 插件 API（首版）

```lua
events.on("clipboard.captured", function(ctx)
  if ctx.item.type == "text" then
    if ctx.item.text:match("github.com") then ctx:add_tag("github") end
    if looks_sensitive(ctx.item.text) then
      ctx:set_sync(false)
      ctx:set_expire("30s")
    end
  end
end)
```

| 能力 | 方法 | 备注 |
|---|---|---|
| 读内容 | `ctx.item.type/text/formats/origin/source_app` | 文件只给名称和大小，不给路径 |
| 改内容 | `ctx:set_text(s)` | 首版只支持文本；HTML/图片/文件只读 |
| 标签 | `ctx:add_tag(t)`、`ctx:remove_tag(t)` | 数量与长度有上限 |
| 同步 | `ctx:set_sync(bool)` | 见 2 |
| 过期 | `ctx:set_expire(duration)` | 到期删除历史记录及关联文件 |
| 通知 | `notify(msg)` | 需宿主支持；失败静默 |

**首版不提供**：网络请求、本地存储、`register_action`/Quick Panel 动作（图中标注“需权限”或属于 UI）。这些需要权限模型，另立决策。

### 6. 插件的加载与配置

- 插件文件放在 `$UNIC_HOME/plugins/*.lua`，按文件名排序执行；是否启用在 `config.toml` 的 `[plugins]` 中声明，未声明的不加载。
- 沿用 0002：目录监听+防抖热加载；语法错误的插件保持上一份有效版本并发 `config` 事件；启动时错误则拒绝启动并说明原因。
- 插件不能修改 `config.toml` 中的限制（`limits.*`）与设备配对状态。

### 7. 数据模型与存储

SQLite 新增历史表（本机，不进 wire）：

```text
clip_items(id, hash, origin, origin_device, kind, created_at, updated_at,
           expires_at, persist, sync_state, source_app)
clip_item_parts(item_id, format, data | blob_ref)
clip_item_files(item_id, name, size, blob_ref)
clip_tags(item_id, tag)
```

- 大于阈值的内容与文件存私有目录，表里只存引用；不要求公共调用者提供路径（0003 §2）。
- 过期由定时清理与启动清理处理；只删除 Engine 自己拥有的资源（0002：绝不整目录删除 inbox）。
- 历史容量设上限（条数与字节），超限按最旧、未收藏优先淘汰。
- 全文索引、收藏、集合不在首版。

### 8. 标签与过期不同步

标签和过期时间只在本机历史里。不改 wire，v2 设备互通不受影响；接收方由自己的插件决定自己的标签与过期。

## 待定：历史记录是否进入 Engine（0003）

这件事需要你评估，两种方向都能做，影响如下。

| | A. 历史与流水线在 Engine 核心 | B. 历史留在桌面层 |
|---|---|---|
| 平台一致性 | 各平台共享同一套标签/过期/策略语义 | 每个平台自己实现，语义容易分叉 |
| 公共契约 | 需要增补 `ListHistory`、`GetItem`、`DeleteItem`、事件，并修订 0003 §6 “不承诺历史” 与 engine-contract | 契约不变 |
| 存储 | 核心管 SQLite（0003 已是默认实现）；移动端需验证 | 平台自选 |
| 插件 | Lua 在核心，移动端需验证 gopher-lua 可用性 | 插件只在桌面 |
| 风险 | 公共 API 面扩大，在移动端验证前会过早冻结 | 与“captured→committed”流水线割裂，同步步骤要跨层 |

倾向 **A**，但**以内部接口落地**：流水线与历史先放在 `internal/`（`history.Store`、`pipeline`），不进公开 `unic/engine` 签名；待你评估并更新 0003 后，再决定哪些接口公开。这样不管选 A 还是 B，现在写的代码都不需要推倒。

## 实施顺序

1. **流水线骨架**：把 `watch → broadcast`、`apply` 重构为阶段链 + 内置同步步骤，行为与现在完全一致（回归测试先行）。
2. **历史存储**：`clip_items` 等表、commit 阶段写入、过期清理；`unic history` 查看。
3. **事件总线**：把现有 `Event` 扩为带阶段与 `origin/automation_depth` 的结构，旧 `/v1` 事件形状保留。
4. **Lua 运行时**：沙箱、超时、`events.on`、`ctx` API、热加载；错误与脱敏。
5. **内置扩展**：URL 检测、敏感信息检测、自动过期、内容分类，用同一套 Lua/内部接口实现，作为示例与测试。

## 验收

- `go build ...` 与 `go test -race ./...` 通过。
- 无插件时，同步行为、防回环、0002 配置语义与现在一致。
- 插件：改文本、加标签、`set_sync(false)` 不发送、`set_expire` 到期删除；超时/死循环/抛错的插件不拖垮 Engine，不影响其他插件。
- 沙箱：插件拿不到 `os/io/require`，无法读写文件或联网。
- 防回环：插件改文本写回后不无限循环；深度上限生效。
- 日志、事件、错误中无剪贴板正文。

## 本阶段不做

- 插件网络/存储/权限模型、`register_action` 与 Quick Panel、WASM/Native 插件、可视化规则编辑。
- 全文检索、收藏、集合、历史跨设备同步、标签/过期随内容同步（需改 wire）。
- 可插拔传输层（Iroh/LAN/Relay/BLE，图中第 7 节）：与本决策无关，另议。
- 清空系统剪贴板的“到期清除”：先只删除历史记录，是否清空系统剪贴板另立决策。
