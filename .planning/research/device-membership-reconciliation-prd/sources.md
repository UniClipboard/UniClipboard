# 调研来源：个人设备成员关系核对

## 产品与仓库约束

- `VISION.md`：个人多设备产品、无账号、离线是预期状态、零配置可用、统一 P2P 核心。
- `docs/AGENTS.md`：PRD 只描述问题、目标、用户需求、范围、约束和验收，不预设内部实现。
- `docs/product/2026-08-07-shared-device-refresh-prd.md`：可借鉴“后台继续、完整结果、不得推断成功”的产品表达；其主动刷新流程不作为当前方案。
- `docs/adr/adr-011-offline-first-member-removal-integration.md`：记录旧版桌面端只展示等待设备和本机移除状态的方式，已不足以覆盖新的用户决定。

## Engine 当前行为证据

- `UniClipboard/Engine docs/adr/020-membership-reconciliation-and-user-decisions.md`：普通增加自动应用；未经本机确认的移除等待用户；拒绝后相关设备组彼此隔离但各自继续使用。
- Engine `WorkspaceConvergenceSummary`：当前公开待移除目标、待决定编号、认知不一致设备和需要升级设备。
- Engine `MembershipEvent`：内部保存发起成员身份，说明变化来源是可验证事实，但当前产品结果没有完整提供。
- Engine `DecideMembershipRemoval`：产品可提交接受或拒绝；决定由 Engine 保存并恢复。

## 当前桌面端行为

- `src/pages/DevicesPage.tsx`：设备页已有取消配对、本机被移除、恢复提示、在线/离线状态和设备详情。
- `src/pages/device-status-utils.ts`：旧逻辑依赖“等待离线设备”和移除数量推断，无法表达新的用户决定和设备组分歧。
- `docs/specs/2026-08-08-member-removal-integration-spec.md`：旧验收只覆盖传播等待、本机移除和恢复，不覆盖接受、保留现有设备、版本过旧及分歧恢复。

## 外部参考边界

本轮尝试检索成熟产品的受信设备移除流程，但浏览器检索环境不可用。PRD 未引用未经核实的外部产品行为；当前草案仅基于 UniClipboard 的产品定位、现有用户界面和 Engine 已确认的业务规则。
