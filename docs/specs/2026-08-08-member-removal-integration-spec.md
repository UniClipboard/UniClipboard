# 桌面端接入 Engine 成员收敛状态 —— 实施规范

- **状态**：已实施
- **日期**：2026-08-11
- **对应决策**：`docs/adr/adr-011-offline-first-member-removal-integration.md`
- **上游**：UniClipboard/Engine PR #24，提交 `983fb2562f55fca3838a927f7831ae51eaadf885`

## 完成定义

以下条件同时满足：

1. `uc-engine` 与 `uc-observability-contract` 固定到上述提交；
2. 守护进程 API 与实时通知完整传递 Workspace convergence 状态，包括
   `waitingMemberDeviceIds` 与本机 `removed` 状态；
3. 前端以同一份状态作为唯一来源，实时通知后重新读取当前状态；
4. 设备列表只在 `waitingMemberDeviceIds` 所列设备的条目末尾显示资料更新警告；
5. 已删除旧的独立设备发现、独立成员移除和永久丢失确认入口；
6. OpenAPI 与前端生成代码已更新；
7. 前端构建、守护进程检查，以及状态映射和实时通知测试通过。

## 状态契约

Workspace convergence 是 Engine 对当前 Space 的完整成员收敛快照。客户端不得缓存后自行
合并，也不得用设备连接状态推断等待成员。

| 字段 | 约定 |
| --- | --- |
| `phase` | `locally_applied`、`converging`、`waiting_for_offline_member`、`complete` 或 `recovery_required`。 |
| `waitingMemberDeviceIds` | 仅在当前成员变更尚待上线设备完成资料更新时由 Engine 给出；列表之外的离线设备不应被标记。 |
| `removed` | 本机已被移除的长期事实，独立于 `phase`；重新加入后清除。 |
| `failureCategory` | `recovery_required` 的稳定原因分类，用于展示，不用于客户端重试策略。 |

## 产品展示

- `waiting_for_offline_member`：仅在名单内每个设备条目末尾显示“等待资料更新”图标及说明。
- `removed`：本机条目显示“此设备已被移除”。
- `recovery_required`：本机条目显示需要恢复，不提供本地继续或永久丢失确认操作。
- 其他阶段保持设备自身的在线或离线状态；在线状态不是资料更新状态的替代判断。

## 验证记录

```bash
bun run gen:api
bun run build
npx vitest run src/pages/device-status-utils.test.ts
cargo check -p uc-daemon --bin uniclipd
cargo test -p uc-webserver workspace_convergence_mapping_preserves_complete_engine_state
cargo test -p uc-webserver workspace_convergence_changes_include_the_complete_engine_state
```

## 不做的事

- 不恢复旧的 shared-device refresh 接口或对话框。
- 不在桌面端维护等待设备名单、移除意图或收敛重试。
- 不将所有离线设备统一展示为需要资料更新。
