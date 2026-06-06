# uc-daemon-process 指南

## 定位

`uc-daemon-process` 是 **本地 `uniclipd` daemon 进程管理的瘦原语层**——只关心「怎么
找到 / 拉起 / 标识这个进程」，不含任何业务、网络、GUI 或数据库逻辑。

ADR-008 P5-0 从 `uc-daemon-local` 抽出，目的是切断一条污染依赖边：
`uc-daemon-client` / `uc-cli` 只需进程管理原语，却经
`uc-daemon-local → uc-application → uc-infra` 被迫编译 `iroh` / `diesel`。把这些原语
下沉到一个依赖面极小的 crate 后，client 侧不再拖入整个 app 栈。

## ⚠️ 硬约束：依赖面极小

- ✅ 仅允许轻量依赖：`dirs` / `libc` / `which` / `serde` / `serde_json` / `anyhow` + `std`
- ❌ **禁止** 依赖 `uc-application` / `uc-platform` / `uc-infra`（app 栈）
- ❌ **禁止** 间接拖入 `iroh` / `diesel` / `tokio` / 任何 GUI 框架
- 守门命令：`cargo tree -p uc-daemon-process -e normal` 不得出现上述任一名字

> 路径解析自带：`app_data_root.rs` 用 `dirs` 自洽复刻 `uc-platform` 的 app-data-root
> 计算（`APP_DIR_NAME` / `UC_PROFILE` / `UC_PORTABLE` 规则），成功路径与原 app 栈
> **字节一致**，故迁移属零行为变化。常量是从 `uc-platform`/`uc-application` 复制而来，
> 改动路径策略时两边都要同步（见 `app_data_root.rs` 顶部注释）。

## 模块职责

| 模块 | 职责 |
|---|---|
| `process_metadata` | PID 文件读写 + `DaemonProcessMode` |
| `socket` | loopback HTTP 地址 + daemon token 路径解析 |
| `spawn` | `uniclipd` detached spawn（`setsid` / `DETACHED_PROCESS`）+ 二进制解析 |
| `spawn_contract` | CLI→daemon run-mode / unattended-unlock 环境契约 |
| `app_data_root`（私有） | 自洽的 app-data-root 路径解析，供 `process_metadata` / `socket` 用 |

## 不负责

- ❌ 任何业务规则（pairing / sync / transfer 在 `uc-application`）
- ❌ spawn 的 **编排**（probe→spawn→等健康、spinner / 超时 UX）——在各 shell / CLI
  自己的代码里（如 `uc-cli/src/local_daemon.rs`），并复用 `uc-daemon-local::health_wait`
- ❌ daemon bearer token 持久化 / 健康轮询（仍在 `uc-daemon-local`）

## 与其他 crate 的关系

```
uc-daemon-client  ─── consume ───┐
uc-cli            ─── consume ───┤
uc-daemon-local   ─── consume ───┼──→ uc-daemon-process
（re-export 给下游） ───────────────┘
```

- `uc-daemon-local` 反向依赖并 `pub use` re-export 这四个模块，保 `uc_daemon_local::<module>` 路径兼容
- `uc-daemon-client` / `uc-cli` 直接 `uc_daemon_process::<module>`

## 验证命令

```bash
cargo check -p uc-daemon-process
cargo test -p uc-daemon-process

# 依赖面守门：以下必须全部无输出
cargo tree -p uc-daemon-process -e normal | grep -iE 'iroh|diesel|tokio|tauri|uc-application|uc-platform|uc-infra'
```
