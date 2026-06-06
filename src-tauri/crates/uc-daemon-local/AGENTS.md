# uc-daemon-local 指南

## 定位

`uc-daemon-local` 是 **`uc-desktop` 桌面宿主层的"进程协调工具集"**——
逻辑上属于 desktop 范畴，因为需要被 GUI shell 与 daemon 二进制 **同时**
消费而物理外置成独立 crate。

它处理的是"桌面双进程模型"的进程间协调，包括：

- daemon bearer token 的持久化与读取
- daemon HTTP/IPC socket 路径与连接信息
- daemon 进程元数据（PID 文件读写、`DaemonProcessMode`）
- daemon 健康探测的纯协议契约（`ProbeOutcome` / `DaemonBootstrapError`）
- 健康轮询 helpers（probe 等待健康 / 等待端点消失）
- `uniclipd` 二进制的 **detached spawn**（`setsid` / `DETACHED_PROCESS`）—— CLI
  与 GUI shell 共用（ADR-008 P3：GUI 转纯 client 后自己 spawn 外部 daemon）

## ⚠️ 硬约束：GUI-framework agnostic

与 `uc-desktop` 一致：

- ❌ **禁止依赖任何 GUI 框架**（`tauri` / `iced` / `egui` / `AppKit` 等）
- ❌ 不引入 webview / window / tray API
- ✅ 允许 `tokio` / `tracing` / 文件系统 / 进程操作

整个 crate 的默认编译路径已经是纯 GUI-agnostic 的了——历史上为 Tauri
sidecar 拉起编排准备的 `sidecar-lifecycle` feature 已经在 in-process
化迁移完成后删除。

`spawn` 模块用的是 `std::process::Command` + `setsid`（unix）/
`DETACHED_PROCESS`（windows），**不** 触碰任何 GUI/窗口 API，仍满足硬约束。
ADR-008 P3 起 GUI 转纯 client，自己 detached spawn 外部 `uniclipd`，所以
detached spawn 原语从 `uc-cli` 下沉到此处由 CLI 与 GUI shell 共用。

## 模块职责

本 crate 自有模块：

| 模块 | 职责 |
|---|---|
| `auth.rs` | daemon bearer token 文件持久化（`load_or_create_auth_token`） |
| `contract.rs` | `ProbeOutcome` / `DaemonBootstrapError` / `terminate_local_daemon_pid` |
| `health_wait.rs` | probe-only 的健康轮询（`wait_for_daemon_health`、`wait_for_endpoint_absent`） |

进程管理原语已下沉到 thin crate **`uc-daemon-process`**（ADR-008 P5-0），本 crate 经
`pub use uc_daemon_process::{handover, process_metadata, socket, spawn, spawn_contract}` re-export，
故既有 `uc_daemon_local::<module>::*` 路径保持不变：

| 模块（re-export 自 `uc-daemon-process`） | 职责 |
|---|---|
| `process_metadata` | PID 文件读写 + `DaemonProcessMode` |
| `socket` | IPC/HTTP socket 路径解析（`try_resolve_daemon_http_addr`） |
| `spawn` | `uniclipd` detached spawn + 二进制解析（`spawn_detached_daemon`、`resolve_daemon_exe_path`） |
| `spawn_contract` | CLI→daemon run-mode / unattended-unlock 环境契约 |
| `handover` | 跨进程受控重启交接存储（`HandoverRecord{target_mode,generation}` 落锁目录，read/write/clear；ADR-008 P5-L L7） |

> **为什么下沉**：`uc-daemon-client` / `uc-cli` 只需进程管理原语，却因 `uc-daemon-local
> → uc-application → uc-infra` 被迫编译 `iroh`/`diesel`。把这些模块移进只依赖
> `dirs`/`libc`/`which`/`serde`/`serde_json`/`anyhow`/`tracing` + `std` 的 `uc-daemon-process` 后，
> client 侧的污染边被切断（`cargo tree -p uc-daemon-client -i iroh` 现为空）。

## 不负责

- ❌ 任何业务规则（pairing / sync / transfer 决策都在 `uc-application`）
- ❌ daemon **业务** 逻辑（daemon 内部的 worker 在 `uc-desktop/src/daemon/`）
- ❌ spawn 的 **编排**（probe→spawn→等健康、spinner / 超时 UX）——那是各 shell
  / CLI 自己的事（如 `uc-cli/src/local_daemon.rs`）。本 crate 只提供 detached
  spawn **原语**（`spawn::spawn_detached_daemon`），不含编排。
- ❌ HTTP/WS API 路由实现（在 `uc-webserver`）

## 与其他 crate 的关系

```
uc-tauri            ─── consume ───┐
uc-macos-native     ─── consume ───┤
（未来其他 GUI shell）─── consume ───┼──→ uc-daemon-local
uc-desktop          ─── consume ───┤
uc-daemon (bin)     ─── consume ───┤
uc-cli              ─── consume ───┘
```

- 上游：被任何"桌面侧进程"消费
- 下游：依赖 `uc-application` (contract types)、`uc-platform`、
  `uc-daemon-contract`、`uc-daemon-process`（进程管理原语 thin crate）

## 边界规则

- 新增功能前问：这事是不是"双进程模型的进程间协调"？
  - 是：放这里
  - 不是、是 daemon 内部业务调度：放 `uc-desktop/src/daemon/`
  - 不是、是 GUI/CLI 框架特定的 spawn 实现：放对应的 shell / CLI crate
- token / socket 路径策略改动要同时检查所有消费方（GUI shell、daemon bin、
  CLI 工具）

## 验证命令

```bash
cargo check -p uc-daemon-local
cargo test -p uc-daemon-local

# 验证不引入 tauri
cargo tree -p uc-daemon-local -e normal | grep -i tauri  # 必须无输出
```
