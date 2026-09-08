# 当前事实

- Desktop 和 Engine 都有预先存在的工作区修改，不能覆盖或纳入本任务提交。
- 全局 Cargo 配置已将 Engine 与观测合同指向相邻 Engine 工作区。
- Desktop 上报偏好已有未提交调整：desktop-telemetry.json 保留 Sentry 开关，应在它上面接入。
- 现有 Engine runtime 安装完整全局 subscriber；Desktop 已安装本机/Sentry，需要可组合能力，不能再次 set_global_default。
- 前端厂商边界已经存在，但没有标准上下文跨进程传播。
