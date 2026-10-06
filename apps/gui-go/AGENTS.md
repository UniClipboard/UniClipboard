# Go GUI 原型规则

- 遵循根 AGENTS.md；当前为隔离原型，不加入生产发布。
- daemon 权威业务继续归 Rust；Go 只负责外壳、进程协调与 native 认证。
- 复用 packages/desktop-host-go，不复制 CLI 的路径/PID/认证实现。
- 日常开发用 `bun wails:dev` / `bun wails:dev:profile <profile>`（开发 profile，需 UNICLIPBOARD_ENV=development）；E2E 与自动化必须使用独立 HOME、唯一 gui-go-* UC_PROFILE、UC_GUI_GO_ISOLATED=1、UC_DISABLE_SYSTEM_CLIPBOARD=1。
- 不编写单元测试；验证真实 native WebView → HTTP/WS → Rust daemon 的端到端路径并保存可复跑工件。
- 测试专用控制面须使用 Go build tag，不出现在正常构建中。
- Wails 优先：每个宿主能力先核查当前固定 Wails 版本（见 `go.mod`）的源码 API 与官方功能；已覆盖需求就直接集成，只保留必要的 Uni 业务语义适配，不手写平台底层实现。能力审计表见 `README.md`「Wails 能力审计」。Wails 不覆盖或与发布格式/签名契约不兼容时，必须写出实际证据，不得猜测。
