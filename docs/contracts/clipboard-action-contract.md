# 剪贴板 Action 契约 v0.1

- Built-in, external Go and Lua plugins can expose Actions. Go Actions accept text, images and files and return text; Lua Actions retain text-only input. Desktop queries the running service and uses the same Action endpoint as the CLI. An Action creates a new history item and can ask the active Desktop adapter to apply it.
- 日期：2026-11-08。
- 依据：[0004](../decisions/proposed/0004-clipboard-pipeline-lua.md)、[Engine 契约](engine-contract.md)、[提交契约](pipeline-contract.md)、[历史契约](history-contract.md)、[插件契约](plugin-contract.md)、[SDK 扩展类型契约](sdk-extension-contract.md)。

## 1. 含义与取舍

Action 是插件声明、由用户在界面上**主动触发**的内容变换，例如“转大写”“去除格式”。它取代早期的“插件自动改写并写回系统剪贴板”：复制的是 A、粘贴出来变成 B，用户无从预期，所以自动阶段（captured/received 处理器）只能打标签、设过期、设 persist/sync，不能改正文，内容原样入历史、原样同步。

Action 从不订阅事件，只有 `RunAction` 才会调用它。Engine 从不写系统剪贴板；写入由宿主在用户动作之后经既有 `BeginApplyHistory` 完成。因此不需要写回 guard、待办任务、自动化深度、插件来源链和回声推断；唯一的回声抑制仍是应用票据的 AppliedToken。

## 2. 声明与列表

插件在静态 Spec 中声明 `Actions []ActionSpec{ID, Title, Accepts, Placements, PostActions}`（规则见[插件契约 §1](plugin-contract.md#1-标识能力与入口)）：

- 全限定键 `Key = pluginID + "/" + actionID`，实例内唯一；ID 为 `[a-z][a-z0-9_-]*`、最多 64 字节；Title 为纯 UTF-8、1..64 字节，不含控制字符。
- `Accepts` is a non-empty unique subset of `{text, image, files}` for Go Actions. Input type must match the selected history item. Results remain text. Lua input remains text-only.
- `Placements` (`placements` in manifests, Lua and RPC) is a **required**, non-empty unique subset of `{history.item_menu, history.toolbar, quick_panel}`. Omitting `placements` or declaring unknown or duplicate values is rejected as `invalid_config`. Semantic placements are shared across platforms and mapped by each host to its native entry point:
  - `history.item_menu`: history item context menu (Desktop list/grid right-click "动作" submenu; Mobile card long-press context menu).
  - `history.toolbar`: history toolbar / detail action bar (Desktop History toolbar "动作" menu; Mobile history detail action bar).
  - `quick_panel`: quick panel action list (Desktop Quick Panel; ignored on hosts without one).
  Hosts render only the placements they support and ignore unsupported ones. CLI lists and executes all enabled Actions regardless of UI placement.
- `PostActions` (`post_actions` in manifests, Lua and RPC) is a **required**, non-empty unique subset of `{copy, notify, present, select, none}` declaring bounded host follow-up behavior after the new history item commits. Omitting `post_actions`, declaring unknown/duplicate values, or combining `none` with any other value is rejected as `invalid_config`:
  - `copy`: automatically apply the new history item to the system clipboard (Desktop passes `Apply: true`; CLI keeps explicit `--apply` control).
  - `notify`: show a lightweight completion/status toast visible across layouts.
  - `present`: open a host result modal/sheet displaying the committed result text (read through the bounded history content endpoint, never logged), plugin provenance, clipboard status, copy/view-item buttons, an explicit user-clicked "Open Link" button when the result is a valid `http://` or `https://` URL (no silent automatic browser launch), and—on desktop hosts—a locally generated QR code (expanded by default, never using external services or logging the URL) when that URL is non-loopback and within scannable length bounds.
  - `select`: select and focus the newly created history item.
  - `none`: commit to history only, with no clipboard write or extra host follow-up UI.
- Optional `TimeoutMS` (`timeout_ms` in manifests and RPC) is 0..300000. Zero uses 10 seconds. Positive values select a bounded Go invocation deadline, including plugin gate wait. Lua budgets remain unchanged.
- 无参数。需要参数的变换首版不提供。

`ListActions(ctx) ([]ActionInfo, error)` 返回当前已启动且具备 action 能力的插件声明（Key、PluginID、ID、Title、Accepts、Placements、PostActions），按 Key 排序。Engine 运行中可用；列表随插件启用状态变化，无需事件，宿主需要时重新查询。

## 3. RunAction

```go
func (e *Engine) RunAction(ctx context.Context, req RunActionRequest) (ActionResult, error)
```

`RunActionRequest{RequestID, Key, HistoryID, CancelOnContext}`，字段类型见[SDK 扩展类型契约](sdk-extension-contract.md)。`ActionResult{ItemID, HistoryID, Phase, SyncDisposition, OperationID, BackendResults, SyncError}`，后几项与 SubmissionResult 同义：结果条目的提交与同步各自独立表达，网络失败不使 action 失败。

**来源校验**：HistoryID 必须是仍存在、未过期的历史条目，类型匹配 Accepts；不存在为 not_found，类型不匹配为 unsupported。来源条目在整个过程中保持不变，包括标签和过期。

**Execution**: Read the selected source and invoke the plugin through `Run`（Go 经 `plugin.run_action`，Lua 经 `plugin:run_action`）。同一插件实例的 action 与处理器共用串行门；Go uses the declared timeout (10 seconds by default, at most 5 minutes). Lua 沿用[Lua 运行时契约](lua-runtime-contract.md)预算。ActionContext 提供只读 Item 快照（含 Text）、`SetText`、`Host.Notify`，没有标签/策略方法。必须恰好调用一次 SetText：未调用、重复调用、非 UTF-8、空串或超出表示/用户限制，均为失败（invalid_argument 或 limit_exceeded）。

**结果落地**：成功时 Engine 以返回文本建立**新条目**，来源类型 `action`（PluginID 为该插件），经与显式提交相同的流水线 commit：
- captured 处理器照常运行（它们只标注，不会形成循环）。
- action 结果总是入历史，处理器设置 persist=false 对它无效，否则宿主无法应用。
- 同步按普通显式提交的策略与各后端设置决定，等同用户自己复制了一份。
- 发出该条目的 history 事件；没有新的事件类型。

Engine 不写剪贴板。宿主拿到 HistoryID 后调用 BeginApplyHistory 写入系统剪贴板，回读并报告结果；回声由 AppliedToken 抑制，不产生第二条历史。

## 4. 失败与幂等

插件失败、超时、panic、取消或结果非法时：不创建条目，不写历史，不发送，不影响来源；错误按稳定错误码返回（timeout/plugin_panic/invalid_argument/limit_exceeded 等），不含插件消息或正文。外部副作用不能回滚。处理器 on_error 策略不适用于 action：失败就是失败，不“继续”。

RequestID 与 CommitContent 同一幂等命名空间：同键同参返回原结果，不再次调用插件；参数（Key、HistoryID）不同返回 conflict。结果条目一经 commit，其后的同步失败进入 SyncError/后端状态，不改变 action 已成功。结果缓存被驱逐后，通过 HistoryID 仍可查询并应用该条目。

## 5. 生命周期与隐私

仅 running/active 且插件已启动时可用；Stop/挂起中的在途 action 被取消，不留条目。全局 sync.enabled=false、无后端或断网不影响 action 与本机应用，只影响其结果的同步。

事件、日志和错误不含正文、路径或票据；Title 之外的 action 数据不进入状态。来源与结果正文只通过插件调用的短期流传递，普通 processor 看不到 action 的输入输出。

## 6. 待实现的契约测试

- Spec validation covers unique keys, ID/Title bounds, non-empty unique Go inputs from text/image/files, text-only Lua input, required non-empty unique `Placements` from `{history.item_menu, history.toolbar, quick_panel}`, and required non-empty unique `PostActions` from `{copy, notify, present, select, none}` with `none` exclusive. Reject missing, unsupported, incompatible or duplicate values. Verify ListActions ordering and plugin lifecycle.
- 来源校验：不存在/过期/非文本；来源条目不变。
- 恰好一次 SetText；空串、非法 UTF-8、超限、未调用、重复调用都不留条目。
- 超时、panic、取消不留条目；Stop/挂起取消在途调用；Lua 预算超限为失败。
- 新条目来源为 action，经 captured 处理器标注；persist=false 被忽略；同步按普通显式提交，网络失败不使 action 失败。
- RequestID 幂等：同键不重复调用插件，换参数 conflict。
- 宿主 BeginApplyHistory 应用结果，AppliedToken 回声不产生第二条历史；同步关闭时本机 action 与应用可用。
- 事件/日志/错误不含正文与路径。

## Local headless host

The [local service contract](local-service-contract.md) defines explicit submissions, transient Received reads,
independent Action application and exclusive desktop adapter generations.
A missing clipboard adapter does not restrict Engine import, commit, receive, history reads or export.
Service restart invalidates local sessions and content leases.

## Go Action source content

Go `ActionContext` also implements `plugin.ActionSource`. `Parts()` returns
ordered `{MIME, File, Name, Size}` metadata for the selected content. Names are
display names, never host paths. `OpenPart(ctx, index)` opens one indexed part at
most once per invocation. External processes may hold at most four open parts.
The existing RPC windows and stream budget apply; file bytes do not enter JSON
control frames. Empty parts return an empty reader without a binary stream.

Engine retains the selected content until invocation completion. Each reader
retains its own reference. Callers close readers; invocation completion,
cancellation, timeout and Engine shutdown revoke outstanding readers. Late opens
fail. Changed or unreadable sources fail the Action. Mid-stream source failure
terminates the external RPC session so a partial stream cannot appear complete.
Source paths and file-reference tokens never cross the Action RPC boundary.

`CancelOnContext` defaults false: caller cancellation stops waiting for admitted
work. True cancels that admitted invocation when the caller context ends. This
matches explicit commit cancellation. CLI and Desktop opt in for user-triggered
Actions. Deduplicated RequestIDs keep the existing result/failure; this does not
promise exactly-once external effects across eviction or service restart.

Only the plugin owns provider-specific upload behavior. A share Action returns
the confirmed public download URL through exactly one `SetText` call. Failed
uploads never create a success history item. A provider effect may survive local
cancellation or a later local commit failure.

Source-open failures and non-cancellation source-read failures are latched in
invocation state, including premature EOF. An Actor cannot turn them into success
by swallowing the error or returning text. The Engine checks that state before
importing a result. Deliberate early part release and invocation cancellation are
cleanup paths, not incomplete-source failures.

Closing an unread or partially read Go source reader uses nested
`action.close_part`; the response confirms that its host reader and live-part
slot are released. Full reads still finish with END. See the
[RPC release contract](plugin-rpc-contract.md#early-release-of-an-action-source-part).
