# Brief: Engine PR #14 桌面端接收控制

**Date:** 2026-07-31
**Status:** Implementation ready; waiting for Engine Release
**Research question:** desktop 应如何完整接入 Engine PR #14 的逐成员接收控制？

## Recommendation

沿用现有成员同步偏好接口，在设备详情页同时展示发送与接收总开关、两套内容类型允许列表；所有更新继续发送局部 patch，并在失败后重新读取 Engine 权威值。Engine 依赖只升级到首个包含合并提交 `9a403d7ed2687902fab52afb1d31c4b9ca746a71` 的不可变 Release 标签。

## Key findings

1. desktop 已有完整的成员同步偏好数据模型、GET/PATCH 接口和乐观更新逻辑，接收字段只是未展示。
2. 当前 `v0.20.0-rc.15` 不包含 PR #14；该 PR 合并后尚无新 Release。
3. Engine 要求更新时只提交发生变化的字段，并以随后查询到的完整偏好作为界面权威值。

## Approach

- 扩展现有 `PeerDetailPanel`，加入接收总开关和接收内容类型。
- 恢复默认值同时重置发送与接收字段。
- 更新失败时保留现有的重新查询行为，并增加用户可见的失败提示。
- Release 出现后统一更新 `Cargo.toml`、`Cargo.lock` 和 Engine 来源校验脚本。

## Constraints

- desktop 只能依赖公开入口 `uc-engine` 和 `uc-observability-contract`。
- 不得锁定 Engine 的浮动分支或开发分支。
- 接收偏好仅作用于本机的当前 P2P 成员，不影响 LAN 兼容线路。
- 恢复接收只影响之后到达的内容。

## Implementation checklist

- [x] 为接收总开关和内容类型补失败测试
- [x] 展示并更新接收设置
- [x] 恢复默认值覆盖发送与接收设置
- [x] 更新全部语言文案和产品文档
- [ ] 升级到包含 PR #14 的 Engine Release
- [x] 通过前端测试、产品构建、Engine 来源校验和界面实测

## Current blocker

截至 `2026-07-31`，Engine 最新 Release 仍为 `v0.20.0-rc.15`，早于 PR #14
合并。desktop 按仓库规则不能锁定 `main` 或未发布提交；待新 Release 发布后再完成依赖升级。

## Alternative considered

直接锁定 Engine `main` 或合并提交虽然可以立即取得行为，但违反 desktop 只消费不可变 Release 的仓库边界，因此不采用。
