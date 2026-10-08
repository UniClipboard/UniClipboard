# Go GUI 宿主规则

- 遵循根 AGENTS.md；本模块是目前唯一的桌面宿主（旧 Tauri 宿主已退役，记录见 `docs/architecture/gui-go-tauri-retirement.md`）；发布流水线尚未完整重建：macOS 应用包的构建、签名、公证、DMG 与更新归档已在 `.github/workflows/build.yml` 的 `package-macos-gui` 作业中实现（`packaging/macos/`，见 `README.md`「macOS 发布构建」），更新签名与隔离验收已接入（见 `docs/architecture/gui-go-updater-signatures.md`），Windows 与 Linux 安装包尚未接入生产，`release.yml` 仍被有意阻塞。
- macOS 发布形态（`release` 标签）无 profile、使用真实数据根与登录钥匙串：只能在一次性机器（GitHub 托管 runner）上运行，本机验收一律用 `packaging/macos/package.py bundle --variant acceptance`（独立标识、独立 HOME 与 profile、文件密钥库）。
- daemon 权威业务继续归 Rust；Go 只负责外壳、进程协调与 native 认证。
- 复用 packages/desktop-host-go，不复制 CLI 的路径/PID/认证实现。
- 日常开发用 `bun wails:dev` / `bun wails:dev:profile <profile>`（开发 profile，需 UNICLIPBOARD_ENV=development）；E2E 与自动化必须使用独立沙箱、唯一 gui-go-* UC_PROFILE、UC_GUI_GO_ISOLATED=1、UC_DISABLE_SYSTEM_CLIPBOARD=1；沙箱在 macOS 是临时 HOME，在 Windows 是 `uc-gui-go-*` 便携目录（`UC_PORTABLE=1`，因为 Windows 不读 HOME，且 daemon 默认写真实 Credential Manager）。会发送真实按键、改变前台窗口或覆盖剪贴板的脚本只能在专用测试主机运行。
- 不编写单元测试；验证真实 native WebView → HTTP/WS → Rust daemon 的端到端路径并保存可复跑工件。
- 测试专用控制面须使用 Go build tag，不出现在正常构建中。
- Wails 优先：每个宿主能力先核查当前固定 Wails 版本（见 `go.mod`）的源码 API 与官方功能；已覆盖需求就直接集成，只保留必要的 Uni 业务语义适配，不手写平台底层实现。能力审计表见 `README.md`「Wails 能力审计」。Wails 不覆盖或与发布格式/签名契约不兼容时，必须写出实际证据，不得猜测。
