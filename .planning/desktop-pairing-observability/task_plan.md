# 桌面配对观测实施

目标：实现 docs/specs/2026-09-06-desktop-pairing-observability-spec.md。保留 Sentry、本机日志、隐私设置；开发 Jaeger 独立开启。

- [进行中] A：核对现有变更，组合进程采集与开发输出。
- [待做] B：标准上下文跨前端/HTTP/Tauri 传播。
- [待做] C：真实加入流程与 Engine 配对生命周期关联。
- [待做] D：失败、取消、恢复、开关、隐私与实际设备验证。
- [待做] 审查、文档与范围内提交；未验证的平台必须明确记录。

Cargo 仅主执行者串行运行，复用共享外置 target。已存在全局本地 Engine patch，不能删掉用户当前适配。
