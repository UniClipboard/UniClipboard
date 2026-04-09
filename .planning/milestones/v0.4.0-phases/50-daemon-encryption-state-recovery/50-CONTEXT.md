# Phase 50: Daemon encryption state recovery on startup - Context

**Gathered:** 2026-03-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Daemon 启动时从磁盘恢复 master key 到 encryption session，解决 daemon 重启后 proof verification 失败的问题。

当前问题：`InMemoryEncryptionSessionPort` 只在内存中保存 MasterKey，daemon 重启后 encryption session 为空，导致所有加密操作（proof verification、剪贴板解密）失败。

本阶段交付：

- daemon 启动时自动调用 `AutoUnlockEncryptionSession` use case，从 keyslot.json + 系统密钥环恢复 master key
- 如果 EncryptionState::Initialized 但恢复失败则拒绝启动
- GUI 模式下同样可复用此 use case（已有）

本阶段不包括：

- 修改加密初始化流程
- 新增密钥轮换机制
- 修改前端加密交互

</domain>

<decisions>
## Implementation Decisions

### 恢复触发时机

- **D-01:** daemon 启动即恢复。DaemonApp::run() 启动时立即检测 EncryptionState，如果已初始化则自动从磁盘/密钥环恢复 master key 到 EncryptionSessionPort。
- **D-02:** 不采用惰性恢复（首次加密操作时才恢复），保证启动后 proof verification 等操作马上可用。

### KEK 获取策略

- **D-03:** 从系统密钥环自动读取 KEK，用 KEK 解包 keyslot.json 中的 wrapped master key。当前 `InitializeEncryption` 已将 KEK 存入密钥环，恢复时只需读出即可，无需用户交互。
- **D-04:** 开发模式下，现有 infra 层的本地文件 fallback（SecureStorage 实现）已支持，无需额外处理。

### 恢复失败处理

- **D-05:** 如果 EncryptionState::Initialized 但恢复失败（keyslot.json 损坏、密钥环 KEK 丢失、解包失败），daemon 拒绝启动并返回错误码退出。
- **D-06:** 不采用降级运行模式（daemon 启动但加密不可用）。加密是核心能力，不可用时不应允许 daemon 运行。

### 职责分层

- **D-07:** 直接复用现有 `AutoUnlockEncryptionSession` use case（位于 uc-app 层），不新建 `RecoverEncryptionSession`。研究阶段发现 `AutoUnlockEncryptionSession` 已完整实现所需功能（EncryptionState 检测、KEK 读取、master key 解包、session 设置），且已有 7 个单元测试覆盖所有边界情况。用户已确认批准复用。
  - _原始决策为"新建 RecoverEncryptionSession use case"，经研究发现功能完全重复后更新。_
- **D-08:** daemon 的 DaemonApp::run() 在 workers 启动前调用此 use case。如果 EncryptionState::Uninitialized 则跳过恢复（首次运行场景）。
- **D-09:** use case 放在 uc-app 层以确保 GUI 模式也可以复用同一恢复逻辑。

### Claude's Discretion

- use case 内部的具体错误类型定义和映射
- DaemonApp::run() 中调用恢复的具体位置（只要在 workers 启动前即可）
- 恢复流程的 tracing span 设计

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Encryption infrastructure

- `src-tauri/crates/uc-core/src/ports/security/encryption_session.rs` — EncryptionSessionPort trait（is_ready, get_master_key, set_master_key, clear）
- `src-tauri/crates/uc-platform/src/adapters/encryption.rs` — InMemoryEncryptionSessionPort 实现
- `src-tauri/crates/uc-core/src/ports/security/encryption_state.rs` — EncryptionStatePort trait
- `src-tauri/crates/uc-infra/src/security/encryption_state_repo.rs` — FileEncryptionStateRepository 实现
- `src-tauri/crates/uc-infra/src/fs/key_slot_store.rs` — JsonKeySlotStore（加载 wrapped master key）
- `src-tauri/crates/uc-infra/src/security/key_material.rs` — DefaultKeyMaterialService（KEK 读写 + keyslot 读写）
- `src-tauri/crates/uc-infra/src/security/encryption.rs` — EncryptionRepository（unwrap_master_key）
- `src-tauri/crates/uc-core/src/security/model.rs` — MasterKey, KEK, WrappedMasterKey 类型定义

### Encryption initialization (参考流程)

- `src-tauri/crates/uc-app/src/usecases/initialize_encryption.rs` — InitializeEncryption use case，恢复流程是其逆操作

### Auto-unlock use case (直接复用)

- `src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs` — AutoUnlockEncryptionSession use case，完整实现恢复逻辑

### Daemon startup

- `src-tauri/crates/uc-daemon/src/app.rs` — DaemonApp::run() 生命周期
- `src-tauri/crates/uc-daemon/src/main.rs` — daemon 入口
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — 依赖注入和 InMemoryEncryptionSessionPort 创建

### Space access proof (受影响的下游)

- `src-tauri/crates/uc-core/src/security/space_access/domain.rs` — SpaceAccessProofArtifact
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — DaemonPairingHost（proof verification 使用 encryption session）

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `DefaultKeyMaterialService`: 已有 `load_kek(scope)` 和 `load_keyslot(scope)` 方法，可直接用于恢复流程
- `EncryptionRepository::unwrap_master_key()`: 已有解包逻辑
- `InMemoryEncryptionSessionPort::set_master_key()`: 已有设置接口
- `FileEncryptionStateRepository::load_state()`: 已有状态检测

### Established Patterns

- use case 模式：`InitializeEncryption` 是同一领域的 use case，恢复流程可参考其结构
- 端口组合：use case 通过 trait 端口组合多个基础设施能力
- CoreUseCases accessor：`auto_unlock_encryption_session()` accessor 已存在于 CoreUseCases 中

### Integration Points

- `DaemonApp::run()`: 调用点——workers 启动前
- `CoreUseCases`: accessor 已注册（`auto_unlock_encryption_session()`）
- `assembly.rs build_core()`: 依赖注入已配置

</code_context>

<specifics>
## Specific Ideas

- 恢复流程本质上是 InitializeEncryption 的逆操作：读 keyslot -> 从密钥环读 KEK -> 用 KEK 解包 wrapped master key -> set_master_key 到 session
- EncryptionState::Uninitialized 时跳过恢复（首次运行，尚未创建加密空间）

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 50-daemon-encryption-state-recovery_
_Context gathered: 2026-03-23_
