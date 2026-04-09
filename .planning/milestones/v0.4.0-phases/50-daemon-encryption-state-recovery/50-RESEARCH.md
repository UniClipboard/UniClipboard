# Phase 50: Daemon Encryption State Recovery - Research

**Researched:** 2026-03-23
**Domain:** Rust / Tauri / uc-daemon encryption session recovery on startup
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** daemon 启动即恢复。DaemonApp::run() 启动时立即检测 EncryptionState，如果已初始化则自动从磁盘/密钥环恢复 master key 到 EncryptionSessionPort。
- **D-02:** 不采用惰性恢复（首次加密操作时才恢复），保证启动后 proof verification 等操作马上可用。
- **D-03:** 从系统密钥环自动读取 KEK，用 KEK 解包 keyslot.json 中的 wrapped master key。当前 `InitializeEncryption` 已将 KEK 存入密钥环，恢复时只需读出即可，无需用户交互。
- **D-04:** 开发模式下，现有 infra 层的本地文件 fallback（SecureStorage 实现）已支持，无需额外处理。
- **D-05:** 如果 EncryptionState::Initialized 但恢复失败（keyslot.json 损坏、密钥环 KEK 丢失、解包失败），daemon 拒绝启动并返回错误码退出。
- **D-06:** 不采用降级运行模式（daemon 启动但加密不可用）。加密是核心能力，不可用时不应允许 daemon 运行。
- **D-07:** 新建 `RecoverEncryptionSession` use case 在 uc-app 层，组合 EncryptionStatePort + KeyMaterialService + EncryptionPort + EncryptionSessionPort。
- **D-08:** daemon 的 DaemonApp::run() 在 workers 启动前调用此 use case。如果 EncryptionState::Uninitialized 则跳过恢复（首次运行场景）。
- **D-09:** use case 放在 uc-app 层以确保 GUI 模式也可以复用同一恢复逻辑。

### Claude's Discretion

- use case 内部的具体错误类型定义和映射
- DaemonApp::run() 中调用恢复的具体位置（只要在 workers 启动前即可）
- 恢复流程的 tracing span 设计

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

## Summary

**关键发现（CRITICAL）：** `AutoUnlockEncryptionSession` use case 已经在 `uc-app/src/usecases/auto_unlock_encryption_session.rs` 中完整实现，功能与 Phase 50 要求的 `RecoverEncryptionSession` 完全一致：

1. 检测 `EncryptionState` — Uninitialized 则返回 `Ok(false)` 跳过，Initialized 则继续
2. 从 `KeyMaterialPort` 加载 keyslot（包含 wrapped master key）
3. 从 `KeyMaterialPort` 加载 KEK（从系统密钥环）
4. 通过 `EncryptionPort::unwrap_master_key()` 解包 master key
5. 通过 `EncryptionSessionPort::set_master_key()` 写入 session

`CoreUseCases` 已有 `auto_unlock_encryption_session()` accessor。Tauri 层的 `encryption.rs` 命令（`unlock_encryption_session`）已经调用它。

**缺失的唯一一件事：** daemon 的 `DaemonApp::run()` 和 `main.rs` 从未调用这个 use case。Phase 50 的主要工作是将这个现有 use case 接入 daemon 启动路径。

决策 D-07 说"新建 RecoverEncryptionSession use case"，但这个 use case 实质上已经存在（名称为 `AutoUnlockEncryptionSession`）。规划时应直接复用此 use case，而非重新创建一个功能相同的新 use case。

**Primary recommendation:** 在 `DaemonApp::run()` 的 workers 启动前调用 `runtime.usecases().auto_unlock_encryption_session().execute()`，失败时通过 `anyhow::bail!` 拒绝启动。

## Standard Stack

### Core（项目已有，无需新增依赖）

| 库            | 版本      | 用途                                                                           | 来源   |
| ------------- | --------- | ------------------------------------------------------------------------------ | ------ |
| `uc-app`      | workspace | AutoUnlockEncryptionSession use case                                           | 已存在 |
| `uc-core`     | workspace | EncryptionSessionPort、EncryptionStatePort、KeyMaterialPort                    | 已存在 |
| `uc-infra`    | workspace | DefaultKeyMaterialService、EncryptionRepository、FileEncryptionStateRepository | 已存在 |
| `uc-platform` | workspace | InMemoryEncryptionSessionPort、SecureStorage                                   | 已存在 |
| `tracing`     | workspace | info_span!、.instrument()                                                      | 已存在 |
| `anyhow`      | workspace | 错误传播                                                                       | 已存在 |
| `thiserror`   | workspace | 错误类型定义                                                                   | 已存在 |

**无需安装新依赖。**

## Architecture Patterns

### 关键发现：AutoUnlockEncryptionSession 已存在

`uc-app/src/usecases/auto_unlock_encryption_session.rs` 是 Phase 50 需要的完整 use case：

```rust
// 文件：src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs
pub struct AutoUnlockEncryptionSession {
    encryption_state: Arc<dyn EncryptionStatePort>,
    key_scope: Arc<dyn KeyScopePort>,
    key_material: Arc<dyn KeyMaterialPort>,
    encryption: Arc<dyn EncryptionPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
}

// execute() 返回：
// Ok(true)  - 恢复成功
// Ok(false) - Uninitialized，跳过（首次运行）
// Err(_)    - 恢复失败（keyslot 损坏、KEK 丢失、解包失败）
pub async fn execute(&self) -> Result<bool, AutoUnlockError>
```

### CoreUseCases 已有 accessor

```rust
// 文件：src-tauri/crates/uc-app/src/usecases/mod.rs（已存在）
pub fn auto_unlock_encryption_session(&self) -> crate::usecases::AutoUnlockEncryptionSession {
    crate::usecases::AutoUnlockEncryptionSession::from_ports(
        self.runtime.deps.security.encryption_state.clone(),
        self.runtime.deps.security.key_scope.clone(),
        self.runtime.deps.security.key_material.clone(),
        self.runtime.deps.security.encryption.clone(),
        self.runtime.deps.security.encryption_session.clone(),
    )
}
```

### DaemonApp::run() 调用点

`DaemonApp::run()` 当前结构（`src-tauri/crates/uc-daemon/src/app.rs`）：

```
pub async fn run(self) -> anyhow::Result<()> {
    // 1. Bind RPC socket（fail-fast）
    // ... 当前没有加密恢复调用 ...

    // 2. Start workers   ← 恢复调用必须在这之前
    let mut worker_tasks = JoinSet::new();
    for worker in &self.workers { ... }

    // 3. Main select loop（shutdown signal）
}
```

**插入位置：** 步骤 1（socket 绑定）之后，步骤 2（workers 启动）之前。

### 推荐的 DaemonApp::run() 修改模式

```rust
pub async fn run(self) -> anyhow::Result<()> {
    info!("uniclipboard-daemon starting");

    // 1. Bind RPC socket FIRST (fail-fast before starting workers)
    check_or_remove_stale_socket(&self.socket_path).await?;
    let listener = UnixListener::bind(&self.socket_path)?;
    // ... auth token, pid guard, api_state setup ...

    // 1.5 Recover encryption session (fail-fast: must succeed if Initialized)
    let recover_span = info_span!("daemon.startup.recover_encryption_session");
    async {
        let uc = self.runtime.usecases().auto_unlock_encryption_session();
        match uc.execute().await {
            Ok(true) => info!("Encryption session recovered from disk"),
            Ok(false) => info!("Encryption not initialized, skipping recovery"),
            Err(e) => {
                anyhow::bail!("Failed to recover encryption session on startup: {}", e);
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .instrument(recover_span)
    .await?;

    info!("uniclipboard-daemon running, RPC at {:?}", self.socket_path);

    // 2. Start workers
    ...
}
```

### 关于 D-07：是否新建 RecoverEncryptionSession

CONTEXT.md 的 D-07 说"新建 RecoverEncryptionSession use case"，但 `AutoUnlockEncryptionSession` 已经完整实现了完全相同的功能。规划时有两种选项：

**Option A（推荐）：直接复用 `AutoUnlockEncryptionSession`**

- 零新代码风险
- 逻辑已经过测试（有 7 个完整单元测试）
- daemon 和 GUI 共享同一 use case（满足 D-09 的复用目标）
- 仅需修改 `DaemonApp::run()` 添加调用

**Option B：按 D-07 新建 `RecoverEncryptionSession`**

- 等价于重复造轮子（功能完全相同）
- 需要新增 use case 文件、accessor、错误类型
- 增加维护负担

研究建议：**直接复用**。规划阶段应向用户说明 `AutoUnlockEncryptionSession` 已有，询问是否可以直接使用，避免不必要的代码重复。

### 错误处理模式（项目惯例）

项目 CLAUDE.md 明确禁止 `unwrap()` / `expect()`，要求使用 `match` 而非 `if let` 处理应向用户反馈的错误：

```rust
// 恢复失败 - 应用 D-05：拒绝启动
match uc.execute().await {
    Ok(true) => { /* 继续 */ }
    Ok(false) => { /* 跳过，继续 */ }
    Err(e) => {
        // 记录详细错误信息，然后拒绝启动
        tracing::error!("Encryption session recovery failed: {}", e);
        anyhow::bail!("Cannot start daemon: encryption session recovery failed: {}", e);
    }
}
```

### Tracing Span 设计（遵循项目惯例）

参考 `InitializeEncryption` 和 `AutoUnlockEncryptionSession` 的 span 命名：

```rust
// 在 DaemonApp::run() 中
let span = info_span!("daemon.startup.recover_encryption_session");
async { ... }.instrument(span).await?;

// use case 内部已有
let span = info_span!("usecase.auto_unlock_encryption_session.execute");
```

## Don't Hand-Roll

| 问题                       | 不要自己实现           | 使用已有实现                              | 原因                        |
| -------------------------- | ---------------------- | ----------------------------------------- | --------------------------- |
| 从 keyslot 恢复 master key | 自定义解析逻辑         | `AutoUnlockEncryptionSession::execute()`  | 已完整实现，有 7 个单元测试 |
| KEK 读取                   | 直接读取 SecureStorage | `KeyMaterialPort::load_kek()`             | 处理开发模式 fallback       |
| master key 解包            | 自定义 XChaCha20       | `EncryptionPort::unwrap_master_key()`     | 已处理所有算法变体          |
| session 写入               | 直接操作内存           | `EncryptionSessionPort::set_master_key()` | 保证线程安全、zeroize       |
| 加密状态检测               | 直接检查文件           | `EncryptionStatePort::load_state()`       | 已处理文件不存在等边界情况  |

## Common Pitfalls

### Pitfall 1：错误位置（最重要）

**What goes wrong:** 在 workers 启动后才调用恢复，导致 proof verification 在恢复完成前就开始失败。
**How to avoid:** 恢复调用必须在 `worker_tasks.spawn(...)` 循环之前完成。
**Warning signs:** 看到 `EncryptionError::NotInitialized` 日志在 daemon 启动后立即出现。

### Pitfall 2：忽略 Uninitialized 状态

**What goes wrong:** 把 `Ok(false)`（Uninitialized）当作错误处理，导致首次运行 daemon 无法启动。
**How to avoid:** `Ok(false)` 是正常路径（首次运行，尚未创建加密空间），不应返回错误。
**Warning signs:** 新设备上 daemon 无法启动。

### Pitfall 3：错误提示不清晰

**What goes wrong:** 只记录 `error!("recovery failed")`，没有说明原因。
**How to avoid:** 记录 `AutoUnlockError` 的完整错误链（`KeySlotLoadFailed`、`KekLoadFailed`、`UnwrapFailed` 等）。
**Warning signs:** 用户反馈"daemon 无法启动"但日志不说明原因。

### Pitfall 4：DaemonApp::run() 的所有权问题

**What goes wrong:** `self.runtime` 在调用 use case 后被移动，导致后续使用出错。
**How to avoid:** `CoreRuntime` 已经是 `Arc<CoreRuntime>`，`usecases()` 返回借用。在调用前 `self.runtime.clone()` 或直接 `self.runtime.usecases()`（不消耗所有权）。
**Warning signs:** 编译错误 `use of moved value`。

### Pitfall 5：未将 DaemonApp 中已有的 run() 步骤对应

**What goes wrong:** 修改 `run()` 时破坏现有逻辑顺序（pid guard、api_state setup）。
**How to avoid:** 在现有步骤 1 完成（socket bind、auth token、pid guard、api_state、event emitter 设置）之后、步骤 2（workers 启动）之前插入恢复逻辑。

## Code Examples

### 已有 use case 的完整调用流程

```rust
// AutoUnlockEncryptionSession::execute() 的内部逻辑（已实现）：
// 1. encryption_state.load_state() → Uninitialized: return Ok(false)
// 2. key_scope.current_scope()
// 3. key_material.load_keyslot(&scope)
// 4. keyslot.wrapped_master_key.ok_or(MissingWrappedMasterKey)?
// 5. key_material.load_kek(&scope)
// 6. encryption.unwrap_master_key(&kek, &wrapped.blob)
// 7. encryption_session.set_master_key(master_key)
// → return Ok(true)
```

### daemon main.rs 不需要修改

`main.rs` 目前在 tokio 运行时启动前完成所有同步 wiring，然后在 `rt.block_on(daemon.run())` 中异步运行。恢复逻辑在 `DaemonApp::run()` 内部完成，不需要修改 `main.rs`。

### Tauri 层的参考实现（encryption.rs 中已有）

```rust
// 文件：src-tauri/crates/uc-tauri/src/commands/encryption.rs
// 第 123 行开始（Tauri 层已有的调用模式）
let uc = runtime.usecases().auto_unlock_encryption_session();
match uc.execute().await {
    Ok(true) => { /* session unlocked, then ensure_ready() */ }
    Ok(false) => { /* encryption not initialized, skip */ }
    Err(err) => { /* emit session failed event */ }
}
```

Daemon 层的调用比 Tauri 层更简单：不需要 emit 事件给前端，直接 bail 即可。

## State of the Art

| 当前状态                                          | Phase 50 之后                                 |
| ------------------------------------------------- | --------------------------------------------- |
| daemon 启动后 encryption session 为空             | daemon 启动时自动从磁盘/密钥环恢复            |
| proof verification 在 daemon 重启后必然失败       | proof verification 在 daemon 启动后可立即使用 |
| `AutoUnlockEncryptionSession` 仅被 Tauri 层调用   | daemon 和 Tauri 层共享同一 use case           |
| Tauri GUI 重启不影响加密（session 随 Tauri 重建） | daemon 重启后 session 自动恢复                |

## Open Questions

1. **D-07 的解读：新建 vs 复用**
   - 已知：`AutoUnlockEncryptionSession` 功能完全满足需求
   - 不清楚：用户是否坚持新建一个名为 `RecoverEncryptionSession` 的 use case
   - 建议：规划时直接复用现有 use case，在计划说明中注明此发现，由用户决定是否需要重命名

2. **daemon 启动失败的退出码**
   - 已知：`DaemonApp::run()` 返回 `anyhow::Result<()>`，`main.rs` 中 `rt.block_on(daemon.run())?` 会将错误传播到 `main()` 并以非零退出码退出
   - 不清楚：是否需要特定的退出码（目前 anyhow 错误会导致进程以退出码 1 退出）
   - 建议：使用默认的 anyhow 错误传播，退出码 1 表示启动失败，与项目现有行为一致

## Environment Availability

步骤 2.6 跳过 — Phase 50 是纯代码修改，无外部依赖。所有基础设施（系统密钥环、keyslot.json）已在 Phase 46 及之前阶段建立。

## Validation Architecture

**nyquist_validation 未设置为 false，测试验证适用。**

### Test Framework

| Property           | Value                                               |
| ------------------ | --------------------------------------------------- |
| Framework          | cargo test (Rust unit tests)                        |
| Config file        | src-tauri/Cargo.toml（workspace）                   |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon`           |
| Full suite command | `cd src-tauri && cargo test -p uc-app -p uc-daemon` |

### Phase Requirements → Test Map

| 行为                                     | 测试类型                | 命令                                                                                |
| ---------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------- |
| Initialized: 恢复成功，daemon 继续运行   | unit                    | `cd src-tauri && cargo test -p uc-app auto_unlock`                                  |
| Uninitialized: 跳过恢复，daemon 正常启动 | unit                    | `cd src-tauri && cargo test -p uc-app auto_unlock_returns_false_when_uninitialized` |
| Initialized + 恢复失败: daemon 拒绝启动  | unit                    | `cd src-tauri && cargo test -p uc-app auto_unlock_propagates`                       |
| daemon run() 调用恢复逻辑                | integration（手动验证） | 启动 daemon 后检查日志                                                              |

### Wave 0 Gaps

现有 `AutoUnlockEncryptionSession` 已有 7 个单元测试，覆盖了所有边界情况。

如果规划决定复用现有 use case，Wave 0 无新测试文件需要创建。

如果规划决定新建 `RecoverEncryptionSession`，则需要：

- `src-tauri/crates/uc-app/src/usecases/recover_encryption_session.rs`（含单元测试）

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs` — 完整的 use case 实现，已有 7 个单元测试
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — CoreUseCases::auto_unlock_encryption_session() accessor（第 277-285 行）
- `src-tauri/crates/uc-daemon/src/app.rs` — DaemonApp::run() 完整代码，确认当前无恢复调用
- `src-tauri/crates/uc-daemon/src/main.rs` — daemon 入口，确认无恢复调用
- `src-tauri/crates/uc-tauri/src/commands/encryption.rs` — Tauri 层调用 auto_unlock 的参考模式（第 112-163 行）
- `src-tauri/crates/uc-core/src/ports/security/encryption_session.rs` — EncryptionSessionPort trait
- `src-tauri/crates/uc-infra/src/security/key_material.rs` — DefaultKeyMaterialService（load_kek、load_keyslot）
- `src-tauri/crates/uc-app/src/usecases/initialize_encryption.rs` — 参考流程（恢复是其逆操作）

### Secondary (MEDIUM confidence)

- `.planning/phases/50-daemon-encryption-state-recovery/50-CONTEXT.md` — 用户决策（D-01 到 D-09）
- `.planning/STATE.md` — 项目历史决策记录

## Metadata

**Confidence breakdown:**

- 现有代码状态（AutoUnlockEncryptionSession 已存在）: HIGH — 直接读取了源代码
- 调用点（DaemonApp::run()）: HIGH — 直接读取了源代码，确认无恢复调用
- 架构模式: HIGH — 参考了 Tauri 层已有的调用模式
- 错误处理: HIGH — 直接读取了现有 AutoUnlockError 错误类型

**Research date:** 2026-03-23
**Valid until:** 2026-04-23（代码稳定，30 天内有效）
