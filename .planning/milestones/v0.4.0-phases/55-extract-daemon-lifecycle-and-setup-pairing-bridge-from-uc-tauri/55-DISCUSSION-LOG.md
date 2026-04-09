# Phase 55: extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions captured in CONTEXT.md — this log preserves the analysis.

**Date:** 2026-03-24
**Phase:** 55-extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri
**Mode:** discuss
**Areas discussed:** Crate target, terminate_local_daemon_pid handling, test migration, migration strategy

---

## Discussion Summary

### 1. Crate Target

**Question:** daemon_lifecycle.rs 应该移到哪个 crate？

- A. uc-daemon-client（推荐）
- B. uc-daemon
- C. 新建独立 crate

**User's choice:** A — uc-daemon-client

**Rationale:** 与 `DaemonConnectionState` 同 crate，简化依赖管理，uc-tauri 已经是 uc-daemon-client 的使用者。

---

### 2. terminate_local_daemon_pid Handling

**Question:** daemon_lifecycle.rs 依赖 run.rs 中的 terminate_local_daemon_pid，如何处理？

- A. 一起移到 uc-daemon-client（推荐）
- B. 保留在 uc-tauri，用 trait 抽象
- C. 复制

**User's choice:** A — 将 terminate_local_daemon_pid 一起移到 uc-daemon-client

**Rationale:** 函数本身零依赖（仅 std::process::Command），移到 uc-daemon-client 后 daemon_lifecycle 完全自包含。

---

### 3. Test Migration

**Question:** daemon_lifecycle.rs 内的 #[cfg(test)] 单元测试如何处理？

- A. 随模块一起移到 uc-daemon-client（推荐）
- B. 留在 uc-tauri 独立测试文件

**User's choice:** A — 随模块一起移到 uc-daemon-client

**Rationale:** uc-daemon-client 已有测试模块模式。

---

### 4. Migration Strategy

**Question:** 迁移模式：每步一提交还是批量一次性？

- A. 每步一提交（推荐）
- B. 批量一次性

**User's choice:** A — 每步一提交

**Rationale:** Phase 54 验证了此模式在多步骤迁移中的可审查性和 revert 粒度。

---

## Key Findings (not questioned)

### setup_pairing_bridge.rs Is Dead Code

Grep 全文确认：`setup_pairing_bridge.rs` 零外部调用方（main.rs 已使用 uc-daemon-client 版本）。直接删除即可。

### terminate_local_daemon_pid Semantic Safety

daemon workers 是 Tokio async tasks，不是 OS 子进程。`child.kill()` 与 `kill -TERM $pid` 在此场景下语义等价，迁移安全。

---

## Auto-Resolved

N/A — all questions discussed with user.

---

## External Research

N/A — all findings from codebase analysis.
