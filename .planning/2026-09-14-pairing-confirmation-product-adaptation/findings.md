# Findings: Pairing Confirmation Product Adaptation

## Starting State
- 工作区起始已有未提交改动：临时本地 Engine override，以及 `DeviceTrustContext` 的刷新串行化和对应测试。
- 这些改动属于当前产品适配方向，继续审查和完成；不把临时本地路径视为正式升级方式。
- 当前分支名 `worktree/calm-field-03d1` 没有表达业务含义，提交前按分支规则处理。

## Contract Inventory
- Engine 的公开设备关系已经提供可选 `pairing_confirmation`，稳定值为 `awaiting_peer_confirmation`、`unconfirmed`、`confirmed`；字段缺失代表旧版本或不适用，Desktop 不应自行补算。
- Desktop 当前 daemon DTO 和投影尚未携带该字段，生成的 TypeScript 类型也没有它；缺口位于公开传输边界，不在 Engine 领域规则内。
- 设备列表现有展示入口集中在 `getDeviceTrustStatus`，适合把 Engine 状态投影为统一行状态；详情页已经复用同一个 `DeviceRowStatus`，无需另建状态来源。
- Engine 状态变化仍通过设备信任快照的 revision 刷新；工作区已有的串行化改动用于避免重复或乱序事件让旧结果覆盖新结果。
