# uc-daemon-local 指南

## 定位

`uc-daemon-local` 是 **`uc-desktop` 桌面宿主层的"进程协调工具集"**——
逻辑上属于 desktop 范畴，因为需要被 GUI shell 与 daemon 二进制**同时**
消费而物理外置成独立 crate。

它处理的是"桌面双进程模型"的进程间协调，包括：

- daemon bearer token 的持久化与读取
- daemon HTTP/IPC socket 路径与连接信息
- daemon 进程元数据（PID 文件读写）
- daemon 启动协调（健康探测 + sidecar 拉起 hook 编排）
- daemon owned-by-GUI 状态跟踪与退出清理

## ⚠️ 硬约束：GUI-framework agnostic

与 `uc-desktop` 一致：

- ❌ **禁止依赖任何 GUI 框架**（`tauri` / `iced` / `egui` / `AppKit` 等）
- ❌ 不引入 webview / window / tray API
- ✅ 允许 `tokio` / `tracing` / 文件系统 / 进程操作

`tauri-plugin-shell` 出现在 Cargo.toml 里**仅以 optional + feature gate 形式**，
不在默认编译路径上：

```toml
[features]
sidecar-lifecycle = ["dep:libc", "dep:tauri-plugin-shell", ...]
```

`sidecar-lifecycle` feature 是为 Tauri shell 准备的——它需要用
`tauri-plugin-shell` 拉 sidecar 子进程。这个 feature 的设计意图就是：

- `uc-tauri` 启用 `sidecar-lifecycle`
- `uc-daemon`（daemon 二进制本身）**不启用**
- 未来 `uc-macos-native` shell 不启用，自己用 `std::process::Command` 或
  `NSTask` 实现 spawn

如果未来要支持非 Tauri 的 sidecar 拉起，应**新增 feature**（如
`sidecar-std`、`sidecar-nstask`），而不是把 `tauri-plugin-shell` 升级
为默认依赖。

## 模块职责

| 模块 | 职责 | feature gate |
|---|---|---|
| `auth.rs` | daemon bearer token 文件持久化（`load_or_create_auth_token`） | 默认 |
| `process_metadata.rs` | PID 文件读写 | 默认 |
| `socket.rs` | IPC/HTTP socket 路径解析（`try_resolve_daemon_http_addr`） | 默认 |
| `daemon_bootstrap.rs` | 启动协调（健康探测 + spawn hook 编排） | `sidecar-lifecycle` |
| `daemon_lifecycle.rs` | owned daemon 状态、退出清理（`GuiOwnedDaemonState`） | `sidecar-lifecycle` |

## 不负责

- ❌ 任何业务规则（pairing / sync / transfer 决策都在 `uc-application`）
- ❌ daemon **业务**逻辑（daemon 内部的 worker 在 `uc-desktop/src/daemon/`）
- ❌ GUI 框架特定的 sidecar spawn 实现细节——desktop / shell 通过
  `bootstrap_daemon_connection_with_hooks` 注入自己的 spawn hook
- ❌ HTTP/WS API 路由实现（在 `uc-webserver`）

## 与其他 crate 的关系

```
uc-tauri            ─── consume ───┐
uc-macos-native     ─── consume ───┤
（未来其他 GUI shell）─── consume ───┼──→ uc-daemon-local
uc-desktop          ─── consume ───┤
uc-daemon (bin)     ─── consume ───┘
```

- 上游：被任何"桌面侧进程"消费
- 下游：依赖 `uc-application`（contract types）、`uc-platform`、
  `uc-daemon-contract`

## 边界规则

- 新增功能前问：这事是不是"双进程模型的进程间协调"？
  - 是：放这里
  - 不是、是 daemon 内部业务调度：放 `uc-desktop/src/daemon/`
  - 不是、是 GUI 框架特定的 sidecar 拉起：放 `uc-tauri` 或对应 shell
- 新增的 spawn 实现要走 hook 注入模式，不要在这里硬编码 Tauri 调用
- token / socket 路径策略改动要同时检查所有消费方（GUI shell、daemon bin、
  CLI 工具）

## 验证命令

```bash
cargo check -p uc-daemon-local
cargo check -p uc-daemon-local --features sidecar-lifecycle
cargo test -p uc-daemon-local

# 验证默认构建不引入 tauri
cargo tree -p uc-daemon-local -e normal | grep -i tauri  # 必须无输出
# 验证 sidecar-lifecycle 才会引入 tauri-plugin-shell
cargo tree -p uc-daemon-local --features sidecar-lifecycle -e normal | grep tauri-plugin-shell
```
