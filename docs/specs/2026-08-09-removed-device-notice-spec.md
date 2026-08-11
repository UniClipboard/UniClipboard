# 被移除设备通知方案（已废止）

- **状态**：已废止，仅保留历史背景
- **日期**：2026-08-09
- **替代方案**：Workspace convergence

本文原计划通过 `MemberRemovalDto.removed` 和独立移除通知链路向各端传播成员移除状态。该计划已被 Workspace convergence 设计完整替代，不再作为实施依据。

当前唯一权威来源是 Engine 提供的 Workspace convergence 快照。成员移除、等待离线成员、恢复要求以及本机是否已被移除，都由同一份快照表达；查询、事件、命令行和界面只投影这份状态，不再维护或传播独立的移除状态。

后续工作不得恢复本文旧方案中的 `MemberRemovalDto.removed` 传播链路，也不得在桌面端建立平行成员状态。历史上的旧接口、旧事件和旧测试应直接删除，以 Workspace convergence 的完整状态和端到端收敛结果作为验收依据。
