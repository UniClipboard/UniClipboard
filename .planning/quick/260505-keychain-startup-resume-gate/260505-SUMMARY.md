---
quick_id: 260505-keychain-startup-resume-gate
status: complete
date: 2026-05-05
plans_executed: 1
files_modified:
  - src-tauri/crates/uc-desktop/src/daemon/startup_recovery.rs
---

# Quick Task 260505 — startup_recovery 守卫 try_resume_session（GUI 启动期 keychain 弹窗第三幕）

## 背景

前两个 quick task 已经按规则 #1/#2/#3 收口了 KEK 路径与 iroh 设备身份：

- `260505-keychain-prompts`: `kek_observed` 进程级缓存，压住加密路径重复弹窗
- `260505-iroh-identity-file-storage`: iroh 长期身份脱离 keychain，走 0600 文件
- `260505-iroh-identity-file-storage` 后续: `try_resume_session` 在 session 已 in-memory 时 short-circuit

但用户实测仍有问题：**GUI 启动后没启用 auto unlock、没点 Unlock 按钮，依然弹 keychain。**

## 真凶

`uc-desktop/src/daemon/startup_recovery.rs:57` 在 daemon 后台启动任务里
**无条件**调用 `space_setup.try_resume_session()`：

```text
spawn_startup_recovery
  → recover_encryption_session(auto_unlock=false) → Ok(false)   ← 这步正确
  → space_setup.try_resume_session()                            ← 这步未守卫
    → space_access.try_resume_session(SpaceId::new())
      → adapter.try_resume_session
        → is_ready()? false   (启动期 in-memory session 为空)
        → load_kek()  → ★ macOS 弹窗 ★
```

`SpaceSetupFacade::try_resume_session` 内部只判断 `setup_status.has_completed`，
不判断 `auto_unlock_enabled`。用户已经设过加密口令（has_completed=true），
就直接绕过 auto-unlock 开关，把 KEK 访问下沉到 keychain。

之前在 `DefaultSpaceAccessAdapter::try_resume_session` 加的 `is_ready()`
short-circuit 只对"session 已经在内存中"的二次调用生效——启动期内存为空，
short-circuit 不命中。

## 改动

只动 desktop 启动恢复一处，零业务/公共 API 变化。

`src-tauri/crates/uc-desktop/src/daemon/startup_recovery.rs`（+22/-12）：

把 `match input.space_setup.try_resume_session().await { ... }` 块包进
`if unlocked { ... } else { tracing::info!(...) }`：

```rust
if unlocked {
    match input.space_setup.try_resume_session().await {
        Ok(true)  => { input.space_setup.refresh_presence().await; }
        Ok(false) => tracing::info!(...),
        Err(e)    => tracing::warn!(...),
    }
} else {
    tracing::info!(
        "background unlock: encryption session not unlocked — \
         skipping space_setup resume to avoid keychain prompt"
    );
}
```

## 启动期 keychain 接触次数对照（GUI 启动）

| 场景 | 改动前 | 改动后 |
|---|---|---|
| 已 setup, auto_unlock=false（用户场景） | **1 次（弹窗）** ❌ | 0 次 ✅ |
| 已 setup, auto_unlock=true | 1 次（规则 #2 允许） | 1 次（规则 #2 允许）|
| 未 setup（首次安装） | 0 次（has_completed=false 自然短路）| 0 次 |
| 用户在 GUI 点 unlock | 0 次启动期 + 1 次（规则 #1 允许）| 同 |

## 副作用

- auto_unlock_enabled=false 时，启动期不再自动推进 switch-space migration recovery。
- 缓解：没 KEK 也推不动 migration；用户点 unlock 解出 KEK 后再做也不迟
  （后续可以在 EncryptionFacade::unlock 成功后追加一次 migration_state 查询，
  此处不在本任务范围）。

## 验证

- `cargo check -p uc-desktop` ✅
- `cargo clippy -p uc-desktop --no-deps` ✅（既存 1 个 derivable_impls warning 与本次无关）
- `cargo test -p uc-desktop --lib` ✅ 41/41 PASS

## 与前两个 quick task 的关系

| Quick task | 修的弹窗路径 | 触发条件 |
|---|---|---|
| `260505-keychain-prompts` | KEK get/set 重复访问 | 加密路径同会话内多次操作 |
| `260505-iroh-identity-file-storage` | iroh 身份 keychain entry | 启动期 IrohNodeBuilder::bind |
| `260505-keychain-startup-resume-gate`（本次） | space_setup.try_resume_session → load_kek | daemon startup_recovery，has_completed=true 但 auto_unlock=false |

三者属于不同 root cause、不同代码路径，叠加才完整覆盖"非授权场景下绝不接触 keychain"。
