# Phase 50: Daemon encryption state recovery on startup - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-23
**Phase:** 50-daemon-encryption-state-recovery
**Areas discussed:** 恢复触发时机, KEK 获取策略, 恢复失败处理, 恢复边界与职责

---

## 恢复触发时机

| Option                 | Description                                                                                                                                  | Selected |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 启动即恢复             | DaemonApp::run() 启动时立即检测 EncryptionState::Initialized，自动从磁盘/密钥环恢复 master key。保证 proof verification 等加密操作马上可用。 | ✓        |
| 首次加密操作时惰性恢复 | 启动时不做恢复，直到 encryption_session.is_ready() 返回 false 时才触发恢复。可能加快启动，但需要在多个调用点加恢复分支。                     |          |
| worker 启动前恢复      | 在 DaemonApp::run() 中 workers 启动之前恢复。worker 启动后即可使用加密能力，恢复失败可以选择不启动 worker。                                  |          |

**User's choice:** 启动即恢复
**Notes:** 无额外说明

---

## KEK 获取策略

| Option           | Description                                                                                                                                                   | Selected |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 系统密钥环读取   | 从系统密钥环自动读取 KEK，用 KEK 解包 keyslot.json 中的 wrapped master key。当前 InitializeEncryption 已将 KEK 存入密钥环，恢复时只需读出即可。无需用户交互。 | ✓        |
| 用户重新输入口令 | 要求用户重新输入加密口令，用 Argon2id 派生 KEK。更安全但需要 daemon 有交互能力，对 headless daemon 不友好。                                                   |          |
| 双路回退         | 优先密钥环，失败则等待口令输入。兼顾自动恢复和密钥环不可用场景，但复杂度更高。                                                                                |          |

**User's choice:** 系统密钥环读取，开发模式下从本地文件读取（infra 层已支持）
**Notes:** 用户补充说明开发模式下的 SecureStorage 实现已有本地文件 fallback

---

## 恢复失败处理

| Option          | Description                                                                                                     | Selected |
| --------------- | --------------------------------------------------------------------------------------------------------------- | -------- |
| 降级运行        | daemon 正常启动，但 encryption session 保持空，加密相关操作返回明确错误。日志记录 warn。不阻塞 daemon 启动。    |          |
| 启动失败退出    | 如果 EncryptionState::Initialized 但恢复失败，daemon 拒绝启动并返回错误码退出。强制用户修复加密状态后才能启动。 | ✓        |
| warn + 重试机制 | 启动时 warn 并降级，但后台定期重试恢复（密钥环可能因 unlock 延迟可用）。每 30s 重试一次，成功后停止。           |          |

**User's choice:** 启动失败退出
**Notes:** 无额外说明

---

## 恢复边界与职责

| Option                | Description                                                                                                                                                                                           | Selected |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 新 use case           | 新建 RecoverEncryptionSession use case 在 uc-app 层，组合 EncryptionStatePort + KeyMaterialService + EncryptionPort + EncryptionSessionPort。daemon 启动时调用。符合现有架构——业务流程作为 use case。 | ✓        |
| bootstrap 层内联      | 直接在 uc-bootstrap 的 build_daemon_app 或 build_core 中内联恢复逻辑。简单直接，但破坏分层——bootstrap 不应包含业务流程。                                                                              |          |
| DaemonApp::run() 内联 | 在 DaemonApp::run() workers 启动前内联调用恢复。简单，但恢复逻辑耦合在 daemon 层，GUI 无法复用。                                                                                                      |          |

**User's choice:** 新 use case
**Notes:** 无额外说明

---

## Claude's Discretion

- use case 内部的具体错误类型定义和映射
- DaemonApp::run() 中调用恢复的具体位置（只要在 workers 启动前即可）
- 恢复流程的 tracing span 设计

## Deferred Ideas

None — discussion stayed within phase scope
