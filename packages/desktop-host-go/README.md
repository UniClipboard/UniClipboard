# Go 桌面宿主公共能力

Go CLI 与 Go GUI 共用的 daemon 连接、认证、路径和进程原语。
代码从 apps/cli-go/internal 提取，不改变 Rust daemon 的连接与存储契约。
不包含终端交互、GUI 框架、Engine、数据库或业务规则。

`buildinfo` 的生成入口在此模块：

```sh
cd packages/desktop-host-go
go generate ./buildinfo
```

生成输入仍是仓库根 `Cargo.toml` 与 `crates/uc-daemon-contract/src/lib.rs`。
CLI 保留终端交互与 oneshot 升级流程；共享模块拥有健康分类、进程身份、路径与 native
认证。GUI 当前直接复用健康守卫，不接管 CLI 的终端编排或 Engine 业务。
