# Go 插件 SDK 使用说明

Go 插件是独立程序：宿主（Engine）启动它，通过标准输入/输出上的二进制帧 RPC 通信。插件不被编译进宿主，也不使用 Go 动态库。契约见 [插件契约](../docs/design/plugin-contract.md) 与 [RPC 核心协议](../docs/design/plugin-rpc-contract.md)；可编译示例见 [`examples/emailtag`](examples/emailtag)。

## 写一个插件

需要各端自动生成设置界面时，使用独立的 [`plugin/settings`](../plugin/settings) 从配置 struct 生成静态 schema；可运行示例见 [`examples/settingsdemo`](examples/settingsdemo)，字段/控件约定见[设置契约](../docs/design/plugin-settings-contract.md)。schema 生成是开发期工具；清单与 RPC 自动发布尚未接入。

1. 实现 `plugin.Plugin`（`Spec/Init/Start/Close`），再按需实现能力接口：
   - `plugin.Processor`：`Process(ctx, plugin.ProcessContext)`，在 `clipboard.captured` / `clipboard.received` 时自动**标注**（`AddTag/RemoveTag/SetPersist/SetSync/SetExpire`），不能改正文。
   - `plugin.Actor`：`Run(ctx, plugin.ActionContext)`，仅在用户触发时运行，必须恰好调用一次 `SetText`。需要在 `Spec.Actions` 静态声明。
   - `plugin.Observer`：`OnEvent(ctx, plugin.Event)`，接收 `clipboard.committed/updated/deleted/expired`，只读，事件只带历史 ID。
2. 在 `Spec().Capabilities` 里声明与实现一致的能力（`processor/action/observer`）；不一致会在启动时被拒绝。
3. `main` 里调用 `plugin.Serve(p)`。

## 规则

- `Init` 严格校验配置，未知键返回 `plugin.ErrInvalidConfig`；`Init` 不联网，后台任务从 `Start` 开始。
- 回调必须尊重 `ctx`：处理默认 200ms，超时先取消，1 秒宽限后宿主终止进程。
- 同一实例的回调串行执行。panic 会被 SDK 捕获并映射为稳定错误码。
- 返回的错误文本**不会**跨进程边界，宿主只看到稳定错误码；stderr 被丢弃。不要在任何地方记录剪贴板正文。
- `Host.Notify` 每秒最多 5 次、每条最多 1024 字节，仅用于给用户的短提示。
- 标签与过期只在本机历史中，不同步。

## 订阅与装配

宿主用 `engine.PluginInstance{Process: &engine.PluginProcess{...}}` 装配：给出绝对路径、SHA-256 摘要和期望的 `Spec`；握手中的 Spec 必须与之完全一致。`Events` 为空时 processor 订阅 captured+received，observer 订阅全部事件（committed/updated/deleted/expired、pasted、app.started、app.shutdown）；`pasted` 只表示经宿主 UI 应用历史，不代表所有系统粘贴；`app.*` 的 ItemID 为空；列出时每个已声明的能力至少要有一个对应事件。`Disabled` 的实例只校验描述、不启动。

## 失败与恢复

插件崩溃后按 1/2/4 秒退避自动重启，60 秒内最多 3 次，超限变为 unavailable，之后需 `Engine.RestartPlugin` 显式重启。处理器失败时该次标注被丢弃；`OnError: "block_sync"` 使该条目不同步。

## 构建

```sh
make build        # 产物在 dist/
go test -race ./plugins/...
```
