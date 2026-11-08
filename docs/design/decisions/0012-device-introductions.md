# 0012 · 设备介绍：已配对设备推荐候选

- 状态：已采纳；Engine、存储、插件 RPC（`sync_host.introduce`）与桌面 API/CLI 已实现，macOS 界面未做。
- 日期：2026-11-08。
- 依据：[0008](0008-device-identity-and-backend-bindings.md)、[同步后端契约](../sync-backend-contract.md)。

## 场景

A–B 已配对，B–C 配对后，C 能得知 A 的存在。A 不在线、B 之后被移除、B 不再担保 A，都要有明确处理。

## 决策

1. **介绍只是建议，不授信。** C 的用户必须点“配对”，A 的用户也必须批准，指纹带外核对；不自动配对，也不绕过 0001 的批准原则。
2. **介绍方发送其当前担保的设备列表**（ID、名称、地址），每次会话重发，幂等；离线的设备之后自然补上。
3. **移除不级联。** 介绍方在 C 上被移除/撤销，只丢弃它的担保（仅它担保的候选消失）；已建立的 A–C 保持。B 不再担保某设备时，候选在最后一次担保后 7 天过期。用户忽略、撤销过或已有记录的设备不再推荐；已忽略的可恢复。
4. **限额：** 每条消息 ≤ 64，总计 ≤ 256，每个介绍方 ≤ 128。会话必须是已连接且仍批准、信任修订匹配；自身、介绍方、非法 ID、空地址被丢弃。
5. **产品：** 界面单独分区“可能认识的设备”并显示介绍方名称；用户请求配对后 24 小时内在对方上线时自动重试；全局开关 `sync.introductions` 默认开，同时停止发送与接收。
6. 候选名称与地址未经验证，地址视为秘密，不进日志、事件、错误。

## 接口

- Engine：`ListCandidates`、`PairCandidate`、`IgnoreCandidate`；事件对象类型 `candidate`。
- 桌面：`GET /v1/candidates`、`POST /v1/candidates/{id}/pair`（`backend_id`）、`POST /v1/candidates/{id}/ignore`（`ignored`）；事件 `candidates`；CLI `unic suggestions|suggest-pair|suggest-ignore|suggest-restore`。

## 未做

macOS 界面；真实双子进程互通测试。
