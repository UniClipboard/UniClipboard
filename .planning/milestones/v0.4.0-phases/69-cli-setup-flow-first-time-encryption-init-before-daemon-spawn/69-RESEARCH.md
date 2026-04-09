# Phase 69: CLI Setup Flow — First-Time Encryption Init Before Daemon Spawn - Research

**Researched:** 2026-03-28
**Domain:** CLI UX / Encryption initialization / Daemon lifecycle
**Confidence:** HIGH

## Summary

当用户清空数据目录后首次运行 `setup new-space`（即 `setup` 无子命令时选择"Create new Space"），当前代码在提示口令之前先调用 `ensure_local_daemon_running()`。由于 daemon 在启动过程中执行 `AutoUnlockEncryptionSession`，而加密尚未初始化，这条路径会触发 macOS Keychain 访问提示（甚至可能因 Keychain 条目不存在而报错）。这是一个 UX 缺陷，而非架构问题：加密初始化本身不需要 daemon，`InitializeEncryption` 用例只依赖本地文件系统和 keychain 写入。

**正确期望流程：**

1. 检测加密状态：若 `Uninitialized`，CLI 直接完成初始化（不启动 daemon）
2. 若已 `Initialized`，拒绝再次执行 `new-space`（提示用户加密已就绪）
3. 初始化成功后，告知用户运行 `uniclipboard-cli start`（或等效命令）启动 daemon

**Primary recommendation:** 在 `run_new_space()` 中移除 `ensure_local_daemon_running()` 调用，改为直接使用 `build_cli_runtime()` + `CoreUseCases::initialize_encryption()` 在本地完成加密初始化，初始化后给出 `start` 命令提示，并在已初始化时提前返回错误。

## Project Constraints (from CLAUDE.md)

- 所有 Rust 命令必须从 `src-tauri/` 运行
- 禁用 `unwrap()` / `expect()` 于生产代码
- Tauri 命令需通过 `runtime.usecases().xxx()` 访问，永远不用 `runtime.deps.xxx`
- 错误必须显式处理（`match`），不能 `if let` 静默吞掉
- Tailwind 使用 rem 单位（前端无变更，但列出以备查）
- 测试 Rust 必须从 `src-tauri/` 运行：`cd src-tauri && cargo test -p uc-cli`

## Current State — Code Archaeology

### 问题根因

`src-tauri/crates/uc-cli/src/commands/setup.rs` 中的 `run_new_space()`：

```rust
async fn run_new_space() -> i32 {
    let spinner = ui::spinner("Starting daemon…");
    if let Err(error) = ensure_local_daemon_running().await {   // ← 问题所在
        ui::spinner_finish_error(&spinner, &format!("{error}"));
        return exit_codes::EXIT_DAEMON_UNREACHABLE;
    }
    // 然后通过 HTTP 提交 passphrase 给 daemon
    let client = DaemonHttpClient::new()?;
    client.start_setup_host().await?;        // daemon 转换状态机
    client.submit_setup_passphrase(passphrase).await?;  // daemon 执行 InitializeEncryption
```

问题在于：在 Uninitialized 状态下，daemon 启动时会调用 `AutoUnlockEncryptionSession`，此时 keychain 中没有 KEK，导致访问失败或 macOS 弹窗。

### 已有的正确工具链

**`build_cli_runtime()`** — `uc-bootstrap/src/non_gui_runtime.rs`：

- 无需 daemon 即可构建完整 `CoreRuntime`
- 已被 `space_status` 命令使用（无 daemon 查询加密状态）

**`CoreUseCases::initialize_encryption()`** — `uc-app/src/usecases/mod.rs:256`：

- 接受 `Passphrase`，返回 `Result<(), InitializeEncryptionError>`
- `InitializeEncryptionError::AlreadyInitialized` — 已初始化时报错
- 内部依赖：`EncryptionPort`, `KeyMaterialPort`, `KeyScopePort`, `EncryptionStatePort`, `EncryptionSessionPort`（全部通过 `CoreRuntime.deps.security` 提供）

**`CoreRuntime::encryption_state()`** — `uc-app/src/runtime.rs:102`：

- 返回 `Result<EncryptionState, String>`
- `EncryptionState::Uninitialized` vs `EncryptionState::Initialized`

**`InitializeEncryption::execute(passphrase)`** 的完整流程（`initialize_encryption.rs:111`）：

1. 检查是否已初始化（如是，返回 `AlreadyInitialized`）
2. 获取 key scope（profile_id）
3. 创建 keyslot draft（含 KDF 参数和 salt）
4. 派生 KEK（Argon2 KDF）
5. 生成 MasterKey
6. 用 KEK 包裹 MasterKey（XChaCha20-Poly1305）
7. 持久化 keyslot（JSON 文件 `vault_dir/`）
8. 存储 KEK 到 keyring（macOS Keychain）← 只在此处触发 Keychain 写入
9. 持久化初始化标记（`.initialized_encryption` 文件）
10. 将 MasterKey 存入 session（内存中）

### `Passphrase` 类型

位置：`uc-core/src/security/model.rs`（通过 `use uc_core::security::model::Passphrase`）

```rust
pub struct Passphrase(pub String);
```

### 现有密码输入 UI

`ui::password_with_confirm("New space passphrase", "Confirm passphrase")` — 已存在，返回 `Result<String, String>`。

### CLI main.rs 的 `SetupCommands` 枚举

当前子命令：`Pair`, `Connect`, `Status`, `Reset`。无 `NewSpace` 或 `Start` 子命令。

- 交互式选择 "Create new Space" → 调用 `run_new_space()`（匿名函数路径，不是子命令）
- 需要讨论：`start` 命令是否已存在？答：在当前 CLI `Commands` 枚举中没有 `Start` 命令（只有 `Status`, `Setup`, `Devices`, `SpaceStatus`）

## Architecture Patterns

### 推荐的新 `run_new_space()` 流程

```
run_new_space()
├── build_cli_runtime()                      ← 无 daemon，直接本地
├── runtime.encryption_state()
│   ├── Initialized → 错误提示 + EXIT_ERROR  ← 禁止二次初始化
│   └── Uninitialized → 继续
├── ui::password_with_confirm(...)           ← 提示输入口令
├── CoreUseCases::initialize_encryption().execute(Passphrase(passphrase))
│   ├── Ok(()) → 成功
│   └── Err(AlreadyInitialized) → 提示已初始化
│   └── Err(其他) → 显示错误
└── ui::info("Next step", "run `uniclipboard-cli start` to start the daemon")
```

### 不需要 daemon 的好处

- 无 daemon 启动延迟（旧流程需等待 8 秒超时后才报错）
- 无 Keychain 弹窗（旧流程 daemon 启动时 `AutoUnlockEncryptionSession` 访问不存在的 KEK）
- 不依赖网络/端口（CLI 可在任何环境运行）

### `run_new_space()` 不需要改动的内容

- `prompt_new_space_passphrase()` — 继续使用
- `ui::*` 函数集 — 继续使用
- 错误处理结构

## Standard Stack

### Core

| Library                   | Version | Purpose                              | Why Standard                                 |
| ------------------------- | ------- | ------------------------------------ | -------------------------------------------- |
| `uc-bootstrap` (internal) | —       | CLI runtime bootstrap                | 已有 `build_cli_runtime()`                   |
| `uc-app` (internal)       | —       | Use case访问                         | 已有 `CoreUseCases::initialize_encryption()` |
| `uc-core` (internal)      | —       | `Passphrase`, `EncryptionState` 类型 | 域模型层                                     |
| `indicatif`               | 已有    | CLI spinner/progress                 | 已在 `ui.rs` 封装                            |

### 无需新增依赖

本 Phase 所有工具链均已存在，不需要添加任何新 crate 依赖。

## Don't Hand-Roll

| Problem           | Don't Build        | Use Instead                      | Why                    |
| ----------------- | ------------------ | -------------------------------- | ---------------------- |
| 口令哈希/KEK 派生 | 自定义 Argon2 逻辑 | `InitializeEncryption` use case  | 已有完整实现和测试     |
| 加密状态检查      | 直接读文件         | `runtime.encryption_state()`     | 已通过端口抽象，可测试 |
| Keychain 写入     | 直接调用 keyring   | `InitializeEncryption.execute()` | 已处理 scope、错误映射 |
| CLI 运行时构建    | 手动组装 deps      | `build_cli_runtime()`            | 已有完整组装流程       |

## Common Pitfalls

### Pitfall 1: `run_new_space` 调用时机过早的 daemon 启动

**What goes wrong:** 如果保留 `ensure_local_daemon_running()` 调用，daemon 会在加密未初始化时启动，触发 `AutoUnlockEncryptionSession`，进而访问不存在的 Keychain 条目（macOS 弹窗或错误）。

**Why it happens:** `DaemonApp::run()` 中调用了 `recover_encryption_session()`（Phase 50），它在 Initialized 时加载 KEK，在 Uninitialized 时静默跳过，但路径本身会触发 Keychain 访问请求。

**How to avoid:** 在 `run_new_space()` 中完全移除 `ensure_local_daemon_running()`，改用直接 `build_cli_runtime()`。

### Pitfall 2: `Passphrase` 类型包装

**What goes wrong:** `initialize_encryption().execute()` 接受 `Passphrase`（newtype），而 `ui::password_with_confirm()` 返回 `String`。

**How to avoid:** 用 `Passphrase(passphrase_string)` 包装。

```rust
use uc_core::security::model::Passphrase;
// ...
let passphrase_str: String = prompt_new_space_passphrase()?;
let uc = runtime.usecases().initialize_encryption();
uc.execute(Passphrase(passphrase_str)).await?;
```

### Pitfall 3: `build_cli_runtime()` 可能初始化 tracing

**What goes wrong:** `build_cli_runtime()` 内部调用 `build_core()`，其中有 `init_tracing_subscriber()`（幂等，但会设置全局 subscriber）。

**How to avoid:** 该调用已被标记为幂等（idempotent），多次调用安全。不需要特殊处理。

### Pitfall 4: 二次调用 `run_new_space` 的用户体验

**What goes wrong:** 用户可能在已初始化后误运行 `setup new-space`，期望提示而不是报错退出。

**How to avoid:** 使用清晰的 `ui::error("Space already initialized. Use 'start' to launch the daemon.")` 提示，并返回 `EXIT_ERROR`（而不是 panic 或沉默）。

### Pitfall 5: `run_interactive()` 中选项变更

**What goes wrong:** 交互式引导当前提供"Create new Space"和"Join existing Space"两个选项。"Create new Space"路径（`run_new_space()`）不再需要 daemon，但"Join"路径（`run_join()`）仍需要 daemon 参与发现和 pairing。两者应保持独立。

**How to avoid:** 只修改 `run_new_space()`，不改变 `run_join()` 和 `run_host()` 的 daemon 依赖。

## Code Examples

### 新 `run_new_space()` 骨架

```rust
// Source: 基于现有 space_status.rs 和 initialize_encryption.rs 模式
async fn run_new_space() -> i32 {
    use uc_app::usecases::CoreUseCases;
    use uc_core::security::model::Passphrase;

    // 1. 构建 CLI runtime（不启动 daemon）
    let runtime = match uc_bootstrap::build_cli_runtime(
        Some(uc_observability::LogProfile::Cli)
    ) {
        Ok(r) => r,
        Err(e) => {
            ui::error(&format!("Failed to initialize: {e}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // 2. 检查加密状态
    let state = match runtime.encryption_state().await {
        Ok(s) => s,
        Err(e) => {
            ui::error(&format!("Failed to check encryption state: {e}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    if state == uc_core::security::state::EncryptionState::Initialized {
        ui::error("Space already initialized.");
        ui::info("Hint", "run `uniclipboard-cli start` to launch the daemon");
        return exit_codes::EXIT_ERROR;
    }

    // 3. 提示口令
    let passphrase_str = match prompt_new_space_passphrase() {
        Ok(p) => p,
        Err(e) => {
            ui::error(&e);
            return exit_codes::EXIT_ERROR;
        }
    };

    // 4. 本地初始化加密（无 daemon）
    let spinner = ui::spinner("Creating encrypted space…");
    let uc = CoreUseCases::new(&runtime);
    match uc.initialize_encryption().execute(Passphrase(passphrase_str)).await {
        Ok(()) => {
            ui::spinner_finish_success(&spinner, "Encrypted space created");
        }
        Err(e) => {
            ui::spinner_finish_error(&spinner, &format!("{e}"));
            return exit_codes::EXIT_ERROR;
        }
    }

    // 5. 成功提示
    ui::bar();
    ui::success("Setup complete! Your space is ready.");
    ui::info("Next step", "run `uniclipboard-cli start` to launch the daemon");
    ui::end("");
    exit_codes::EXIT_SUCCESS
}
```

### 检测 `EncryptionState` 的导入

```rust
// uc-core::security::state 通过 uc-core crate 可访问
// 在 uc-cli 中需确认 Cargo.toml 有 uc-core 依赖
use uc_core::security::state::EncryptionState;
```

### 验证 uc-cli 的 uc-core 依赖

```bash
cd src-tauri && grep -A5 '\[dependencies\]' crates/uc-cli/Cargo.toml | head -20
```

## State of the Art

| Old Approach                               | Current Approach           | When Changed | Impact                                 |
| ------------------------------------------ | -------------------------- | ------------ | -------------------------------------- |
| CLI `new-space` 依赖 daemon 完成加密初始化 | CLI 直接本地完成加密初始化 | Phase 69     | 消除 Keychain 误弹窗，加快首次设置速度 |

## Open Questions

1. **`uniclipboard-cli start` 命令是否存在？**
   - What we know: 当前 CLI `Commands` 枚举中有 `Status`, `Setup`, `Devices`, `SpaceStatus`，没有 `Start`
   - What's unclear: Phase 69 的提示语应该是什么命令？是 `start` 还是手动运行 `uniclipboard-daemon`？
   - Recommendation: 本 Phase 仅修复 `new-space` 流程；提示语使用 `uniclipboard-daemon` 或按项目约定命令；添加 `start` 子命令是独立工作，不在此范围

2. **`run_host()` 中也有类似问题？**
   - What we know: `run_host()` 也调用 `ensure_local_daemon_running()`，但它用于 pairing 流程，需要 daemon 参与发现
   - What's unclear: 若设备已初始化但 daemon 未运行，`run_host()` 是否也会触发 Keychain 弹窗？
   - Recommendation: 本 Phase 聚焦 `run_new_space()`；`run_host()` 的未初始化状态应已被 Phase 67 守卫

## Environment Availability

Step 2.6: SKIPPED — 本 Phase 是纯代码修改（CLI 逻辑），无外部工具/服务依赖。所有依赖均通过 Rust crate（`uc-bootstrap`, `uc-app`, `uc-core`）在编译时解析。

## Validation Architecture

### Test Framework

| Property           | Value                                                            |
| ------------------ | ---------------------------------------------------------------- |
| Framework          | Rust `cargo test` (Tokio async)                                  |
| Config file        | `src-tauri/Cargo.toml` workspace                                 |
| Quick run command  | `cd src-tauri && cargo test -p uc-cli`                           |
| Full suite command | `cd src-tauri && cargo test -p uc-cli -p uc-bootstrap -p uc-app` |

### Phase Requirements → Test Map

| Req ID    | Behavior                                          | Test Type | Automated Command                                                       | File Exists? |
| --------- | ------------------------------------------------- | --------- | ----------------------------------------------------------------------- | ------------ |
| REQ-69-01 | Uninitialized 状态下不启动 daemon，直接本地初始化 | unit      | `cd src-tauri && cargo test -p uc-cli -- new_space`                     | ❌ Wave 0    |
| REQ-69-02 | 已 Initialized 时 `new-space` 返回错误提示        | unit      | `cd src-tauri && cargo test -p uc-cli -- new_space_already_initialized` | ❌ Wave 0    |
| REQ-69-03 | 初始化完成后显示 `start` 提示                     | manual    | —                                                                       | manual       |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-cli`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-cli -p uc-bootstrap`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-cli/tests/setup_new_space_cli.rs` — 覆盖 REQ-69-01 和 REQ-69-02（新测试文件）
- [ ] 或在现有 `tests/setup_cli.rs` 中扩展测试

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-cli/src/commands/setup.rs` — 现有 `run_new_space()` 实现
- `src-tauri/crates/uc-app/src/usecases/initialize_encryption.rs` — `InitializeEncryption` 用例完整实现
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — `build_cli_runtime()` 实现
- `src-tauri/crates/uc-app/src/runtime.rs` — `encryption_state()` 方法
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — `CoreUseCases::initialize_encryption()` accessor
- `src-tauri/crates/uc-cli/src/commands/space_status.rs` — 无 daemon 模式使用 `build_cli_runtime()` 的参考实现

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon/src/main.rs` — 确认 daemon 启动时调用 `recover_encryption_session()`（解释为何 daemon 会触发 Keychain 访问）

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — 所有依赖已存在于代码库，无需新增
- Architecture: HIGH — 参考实现（`space_status.rs`）已展示正确模式
- Pitfalls: HIGH — 根因通过代码阅读确认，不依赖文档

**Research date:** 2026-03-28
**Valid until:** 2026-05-01（代码库稳定，无外部 API 依赖）
