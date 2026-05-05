---
quick_id: 260505-iroh-identity-file-storage
status: complete
date: 2026-05-05
plans_executed: 1
files_modified:
  - src-tauri/crates/uc-bootstrap/src/assembly.rs
  - src-tauri/crates/uc-bootstrap/src/space_setup.rs
---

# Quick Task 260505 — iroh 设备身份脱离 macOS Keychain（彻底消除启动期弹窗）

## 背景

前一次 quick task `260505-keychain-prompts` 给 `DefaultSpaceAccessAdapter` 加了 `kek_observed` 进程级缓存，压住了加密路径（`unlock` / `verify_keychain_access` / `try_resume_session.store_kek`）的重复弹窗。但用户实测**首次打开应用、还没创建 new space**就被 macOS 弹 keychain，前次修复完全没覆盖这条路径。

## 真凶

`uc-bootstrap/src/space_setup.rs:219` 把 KEK 用的同一条系统 keychain 也传给了 `IrohIdentityStore`。`IrohNodeBuilder::bind` 在启动期被无条件调用，触发：

```
ensure_secret_key()
  → secure_storage.get("iroh-identity:v1")     ← 文件不存在等价物，但走的是 macOS keychain
  → 无 entry → SecretKey::generate()
  → secure_storage.set("iroh-identity:v1", …)  ← macOS 弹"是否允许 UniClipboard 在 keychain 中保存数据"
```

这条路径**绕过所有加密 gate**：
- 不查 `auto_unlock_enabled`（默认 false 也照样弹）
- 不查 `setup_status.has_completed`（用户没初始化也弹）
- 不查 `keyslot_exists()`（KEK 跟 iroh 身份是不同 keychain entry）

违反用户明确给出的三条规则（点 unlock / 启用 auto-unlock / 设置加密口令）。

## 改动

只动 bootstrap 装配层，业务代码 / 公共 trait / 加密协议零变化。

`src-tauri/crates/uc-bootstrap/src/assembly.rs`（+20）：

- `WiredDependencies` 新增 `iroh_identity_dir: PathBuf` 字段
- `wire_dependencies` 中 `apply_profile_suffix(<app_data>/iroh-identity)` + `create_dir_all`，失败 → `WiringError::SecureStorageInit`

`src-tauri/crates/uc-bootstrap/src/space_setup.rs`（+25/-1）：

- `use uc_platform::file_secure_storage::FileSecureStorage;`
- `IrohIdentityStore::new` 第一参数从 `Arc::clone(&deps.security.secure_storage)`（系统 keychain）改成 `Arc::new(FileSecureStorage::with_base_dir(wired.iroh_identity_dir.clone()))`（0600 文件后端）

## 各 SecureStoragePort 消费方现状

| 消费方 | 后端 | 启动期? |
|---|---|---|
| `KeyMaterialStore`（KEK / KeySlot） | macOS keychain | ❌ 仅 unlock / verify_keychain_access / 用户初始化时 |
| `DefaultKeyMigrationAdapter`（migration_key） | macOS keychain | ❌ 仅 switch-space 时 |
| `IrohIdentityStore`（iroh Ed25519 设备身份） | **文件后端**（本次改动） | ✅ 启动期 `IrohNodeBuilder::bind`，但**零 keychain 接触** |

## 迁移策略（用户要求 "静默 + 零弹窗"）

- 老用户 keychain 中残留的 `iroh-identity:v1` 条目：**不读、不删**
  - 任何 `get` 在某些边缘场景（dev 构建签名漂移、用户主动 deny 过）下可能弹窗 → 不冒险
  - `delete` 同样可能弹窗 → 不冒险
  - 残留条目无害，下次 `factory_reset` 或用户手动清理 keychain 即可
- 文件不存在 → `IrohIdentityStore::ensure_secret_key` 生成新身份
- **代价**：老用户升级后 iroh 设备身份重置，**需要重新与 peer 配对一次**（一次性成本，与用户达成共识）

## 安全权衡（与用户达成共识）

iroh 设备身份是**网络栈的"我是哪台机器"标识**，不是用户秘密：

- **攻击者拿到 iroh 身份能做什么**：冒充该设备发起 iroh 握手；但 channel 加密走 KEK 派生的 proof key（仍在 keychain），冒充连接握手解不出来 → **无法读取 / 解密剪贴板内容**
- **vs Keychain**：损失"同用户其它进程访问需 ACL 提示"的提示层；但 root / 物理访问 / FileVault 锁屏保护对两种方案等价
- **行业实践**：SSH (`~/.ssh/id_ed25519`)、GPG、IPFS、Tailscale 等 P2P 工具的设备身份均用 0600 文件，不用 keychain
- **结论**：风险轻微，trade-off 合理

## 启动期弹窗次数对照

| 场景 | 改动前 | 改动后 |
|---|---|---|
| 全新安装、用户没操作 | iroh `set` × 1 → **弹窗 1 次** | 0 次（文件后端） |
| 全新安装 + 用户初始化加密 | iroh `set` × 1 + KEK `set` × 1 = 2 次 | 1 次（仅 KEK，符合规则 #3） |
| 后续启动（已初始化、auto_unlock=false） | iroh `get`（命中）× 1 = 1 次 | 0 次 |
| 后续启动（已初始化、auto_unlock=true） | iroh `get` + KEK `get` = 2 次 | 1 次（仅 KEK，符合规则 #2） |
| 用户点 Unlock 按钮 | KEK `get` × 1 + 可能的 refresh `set`（前次 quick 已修） = 1 次 | 1 次（仅 KEK，符合规则 #1） |

## 验证

- `cargo check --workspace` ✅
- `cargo clippy -p uc-bootstrap -p uc-infra -p uc-platform --no-deps` ✅（既存 warning 11+3+8 = 22 个，本次改动 0 新增）
- `cargo test -p uc-infra --lib security` ✅ 11/11 PASS
- `cargo test -p uc-platform --lib` ✅ 全部 PASS

## 与前一个 quick task 的关系

- `260505-keychain-prompts`（已合入）：解决加密路径的重复弹窗（KEK get/set 在同会话内幂等）
- `260505-iroh-identity-file-storage`（本次）：解决 iroh 身份导致的"用户没操作就弹"

两者是不同的 root cause，各自必要，互不冗余。
