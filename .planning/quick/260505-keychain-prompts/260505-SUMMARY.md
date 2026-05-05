---
quick_id: 260505-keychain-prompts
status: complete
date: 2026-05-05
plans_executed: 1
files_modified:
  - src-tauri/crates/uc-infra/src/security/space_access_adapter.rs
---

# Quick Task 260505 — 减少 macOS Keychain 多次弹窗

## 问题

首次使用 UniClipboard 时，macOS 会多次弹出 Keychain 授权对话框。诊断结论：进程内同一条 keychain 条目（`UniClipboard` / `kek:v1:<scope>`）被以下三条独立路径串行访问，每条都可能触发独立的授权提示：

1. `try_resume_session` → `load_kek`（启动期静默恢复）
2. `verify_keychain_access` → `load_kek`（前端探测授权状态）
3. `unlock(passphrase)` → `store_kek` 刷新写入（注释自承"保持 keyring 与最新口令对齐"）

加上 `do_first_time_init` 自身的首次 `store_kek`，未签名 / dev 构建上的首次上手用户体感 4–5 次弹窗。

## 改动

只动一个文件：`src-tauri/crates/uc-infra/src/security/space_access_adapter.rs`（+49/-2）。

引入进程级幂等开关 `kek_observed: AtomicBool`，语义为"本进程内已确认 keychain 中存在与本机 keyslot 匹配的 KEK"：

| 路径 | 行为 |
|---|---|
| `do_first_time_init` | `store_kek` 成功 → 置 true；`store_keyslot` 失败回滚路径在 `delete_kek` 之后置 false。 |
| `derive_master_key_for_proof`（pairing joiner）| 同上：`store_kek` 成功 → 置 true；两条回滚路径置 false。 |
| `try_resume_session` | `load_kek` 成功 + `unwrap` 成功 → 置 true（unwrap 成功证明 keychain 中 KEK 与本机 keyslot 匹配）。 |
| `unlock(passphrase)` | `kek_observed == true` 时**跳过** `store_kek` 刷新写入（同 KEK 无信息增量）；否则走原写入路径，成功后置 true。失败仍仅 `warn`，非致命。 |
| `verify_keychain_access` | `kek_observed == true` 直接返回 `Ok(true)`，不访问 keychain；否则按原 `load_kek` 探测，`Ok(_)` 分支置 true 后返回。 |
| `factory_reset` | `delete_kek` 之后**复位 false**（keychain 条目已删除）。 |
| `lock` | **不复位**（keychain 条目不受影响，re-unlock 仍是同 KEK）。 |

## 设计决定

- **位置**：`AtomicBool` 直接挂在 `DefaultSpaceAccessAdapter`，**不**进 `InMemorySession`。`InMemorySession` 关心 in-memory `master_key` 生命周期；keychain ACL 是进程级独立维度，与 `lock()` 解耦。
- **内存序**：`Acquire`/`Release`，单写多读路径下足够。
- **公共边界零变化**：`SpaceAccessPort` trait 签名、`KeyMaterialStore` / `SystemSecureStorage` API、所有调用方（`EncryptionFacade`、`UnlockSpaceUseCase`、webserver handler）一律不动。V1 加密协议字节、keyslot 文件格式、KEK 派生算法不动。
- **未触及**：应用签名 / `tauri.conf.json` / `resolve_service_name()` 的稳定化属于另一类问题（运维 / 构建配置），不在本任务 scope；本次只解决"代码层重复访问"造成的弹窗。

## 预期效果

| 场景 | 改动前弹窗次数 | 改动后弹窗次数 |
|---|---|---|
| 首次使用（无 keyslot） | `do_first_time_init.store_kek` × 1 + `unlock.store_kek refresh` × 1 + 后续 `verify_keychain_access` ≈ 2–3 次 | `do_first_time_init.store_kek` × 1（用户点 Always Allow 后整个进程不再弹） |
| 后续启动 | `try_resume_session.load_kek` × 1 + `verify_keychain_access` × 1 = 2 次 | `try_resume_session.load_kek` × 1 |
| Lock → 重新 unlock | `unlock.store_kek refresh` × 1 | 0（`kek_observed` 在 lock 时不复位）|
| `factory_reset` 后再初始化 | 同首次使用 | 同首次使用（`kek_observed` 已复位为 false） |

## 验证

- `cargo check -p uc-infra` ✅
- `cargo clippy -p uc-infra --no-deps` ✅（17 个 warning 全部为既存，本文件 0 新增）
- `cargo test -p uc-infra --lib security` ✅（11/11 PASS）
- 公共 trait + 调用链零变化，调用方无需改动；fake / mock 实现（用于上层 use case 测试）不依赖 `kek_observed`，无影响。

## 后续

剩余的"代码外"弹窗源（属另一议题，本任务不处理）：

- 应用签名 / signingIdentity 稳定（dev 构建未签名时即便逻辑正确也每次都问）
- `resolve_service_name()` 在 release / dev / `UNICLIPBOARD_PROFILE` 切换时返回值要稳定（不同 service 是不同 keychain 条目）
