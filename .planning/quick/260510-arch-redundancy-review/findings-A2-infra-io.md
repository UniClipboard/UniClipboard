# A2 Findings — uc-infra + uc-webserver + uc-daemon-local + uc-platform

范围：`main..HEAD` 共 +8963 / -1141。
重点新增：`uc-infra/mobile_sync/*` (7 文件)、`uc-webserver/mobile_lan/*` (5 文件)、`uc-daemon-local/{contract,health_wait}.rs`、`uc-platform/migrating_secure_storage.rs`。

## 🔴 必删 / 必改

### 1. `graceful_shutdown_port_reuse.rs` 的文件级注释 stale

`src-tauri/crates/uc-webserver/tests/graceful_shutdown_port_reuse.rs:1-11` 三处把测试定位成 "P1 in-process daemon reload 的契约测试"。方案 C 撤销了 in-process daemon reload, 这条注释已经撒谎。测试本体 (同进程 cancel→rebind 同端口) 对 `app.restart()` + commit `ea09cdd3` 加的 graceful shutdown **仍然有价值** —— `app.restart()` 走 fork+exec, 新进程必须在旧进程释放端口后才能 bind。测试不删，但注释必须重写，否则下一个人看到会以为是死代码顺手干掉。

## 🟡 可削减

### 2. `InMemoryMobileDeviceRepository` pub 暴露但只在自身测试用

`uc-infra/src/mobile_sync/device_repo.rs:28` 是 `pub struct`, `mobile_sync/mod.rs:23` 再 `pub use`。grep 全仓：除自身 12 个 `#[test]` 外 **零调用** (生产路径全部走 `db::repositories::DieselMobileDeviceRepository`, 见 `uc-application/src/usecases/mobile_sync/register_device.rs` 及 `non_gui_runtime.rs`)。违反 `uc-infra/AGENTS.md` "测试用 InMemory 不应外泄 API"。建议移到 `#[cfg(test)] mod tests`, 或挂 `#[cfg(any(test, feature = "test-support"))]`。

### 3. `SharedEndpointInfo` type alias 无人用

`uc-infra/src/mobile_sync/endpoint_info.rs:86` `pub type SharedEndpointInfo = Arc<…>` 全仓 grep 零调用方 (`uc-bootstrap`、`uc-desktop` 都直接写 `Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>`)。可删，省一层认知开销。

注：适配器本体 (`Arc<RwLock<…>>` 单写多读) **不是冗余** —— 即便 daemon 不再 in-process reload, LAN listener 仍能在同进程因 settings PATCH 重启 (`uc-desktop/src/daemon/app.rs:351` 起的 `tokio::spawn` 可被 cancel 再 spawn), 多 reader 仍需要 Arc 共享状态。

### 4. `daemon-local/health_wait.rs:5` 注释提到已删的 feature

注释写 `"其它 shell (不启用 sidecar-lifecycle feature)"`, 但 `uc-daemon-local/AGENTS.md` 明确该 feature 已经在 in-process 化迁移完成后删除。改一行字。

## 已确认非冗余

- **`uc-platform/migrating_secure_storage.rs` (371 行)** 给 0.6→0.7 iroh identity 迁移做一次性搬迁，decorator pattern 干净，28 个测试。属于"用户升级完即可下沉"的兼容代码，当下仍需要。
- **`uc-webserver/mobile_lan/*` (5 文件) vs `api/server.rs`** 是 **两个独立 axum listener** (mod.rs:7-12 解释清楚：鉴权语义 + 绑定面不同), 不是重复。`test_support.rs` 已经 `#[cfg(test)]` (mod.rs:21), 边界正确。
- **`uc-daemon-local/contract.rs::terminate_local_daemon_pid`** 唯一一份进程终止实现，`uc-platform` 没有重复。commit `ea09cdd3` 的 graceful shutdown 在 `uc-tauri/src/commands/restart.rs` 落地，**不** 在 platform / daemon-local 重复 —— 这块设计正确。
- **`uc-infra/src/mobile_sync/{credentials_minter, password_hasher, file_staging, lan_probe}.rs`** 都在 `uc-bootstrap/src/non_gui_runtime.rs:186-194` 装配为生产单实装，有真实消费者。
- **port 抽象 `MobileSyncEndpointInfoPort`** 虽生产只有一个实装，但 4 处 facade / usecase 测试用 `FixedEndpoint` / `ExplodingEndpoint` / `BindFailedEndpoint` mock (`uc-application/src/usecases/mobile_sync/get_settings.rs:201,282,303` 等), port 在测试面被复用，保留合理。
- **`uc-daemon-local/health_wait.rs` + `contract.rs::ProbeOutcome` / `DaemonBootstrapError`** 在 `uc-desktop/src/daemon_probe.rs` 大量复用 (~30 处), 抽离合理。

## 结论

四个 crate 共 ~9k 行新增，**没有发现"整个模块 / 抽象层是冗余"级别的问题**。需要处理三处，都是小手术：

1. **必改**: `graceful_shutdown_port_reuse.rs:1-11` 的注释重写为 "`app.restart()` 端口让渡契约", 否则下一波清理可能误删测试。
2. **可削**: `InMemoryMobileDeviceRepository` 收进 `#[cfg(test)]`, 同时删 `mobile_sync/mod.rs:23` 的 `pub use`。
3. **可削**: 删 `SharedEndpointInfo` type alias + 改 `daemon-local/health_wait.rs:5` 的 stale 注释。

P3 (port 单实装泛滥)、P4 (平台进程工具重复)、P5 (大块死代码 pub) 这三类 **未发现明显问题**, 该有的边界纪律 (`AGENTS.md` 的 GUI-agnostic 红线、`uc-infra` 不向上漏第三方类型) 都守住了。
