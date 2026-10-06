# Go GUI 原型规则

- 遵循根 AGENTS.md；当前为隔离原型，不加入生产发布。
- daemon 权威业务继续归 Rust；Go 只负责外壳、进程协调与 native 认证。
- 复用 packages/desktop-host-go，不复制 CLI 的路径/PID/认证实现。
- 手工运行与 E2E 必须使用独立 HOME、唯一 UC_PROFILE、UC_DISABLE_SYSTEM_CLIPBOARD=1。
- 不编写单元测试；验证真实 native WebView → HTTP/WS → Rust daemon 的端到端路径并保存可复跑工件。
- 测试专用控制面须使用 Go build tag，不出现在正常构建中。
