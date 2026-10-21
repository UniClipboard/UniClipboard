# Go 桌面宿主公共能力

Go CLI 与 Go GUI 共用的 daemon 连接、认证、路径和进程原语。
代码从 apps/cli-go/internal 提取，不改变 Rust daemon 的连接与存储契约。
不包含终端交互、GUI 框架、Engine、数据库或业务规则。

`settings` 包是宿主读取 `GET /settings` 的共享视图，以及跟随 daemon WS 主题 `settings`
的订阅器（`Watcher`）。它不依赖 GUI 框架，所以没有窗口时宿主也能收到设置变化。协议见
`docs/adr/adr-013-desktop-product-settings.md` 第 7 节。真实 daemon 的端到端验证：

```sh
cargo build -p uc-daemon
cd packages/desktop-host-go
UC_E2E_UNICLIPD=$PWD/../../target/debug/uniclipd go test -tags e2e -run TestWatcherAgainstDaemon -count=1 -v ./settings
```

`buildinfo` 的生成入口在此模块：

```sh
cd packages/desktop-host-go
go generate ./buildinfo
```

生成输入仍是仓库根 `Cargo.toml` 与 `crates/uc-daemon-contract/src/lib.rs`。
CLI 保留终端交互与 oneshot 升级流程；共享模块拥有健康分类、进程身份、路径与 native
认证。GUI 当前直接复用健康守卫，不接管 CLI 的终端编排或 Engine 业务。
