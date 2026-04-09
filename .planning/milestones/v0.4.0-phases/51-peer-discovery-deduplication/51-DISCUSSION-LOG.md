# Phase 51: Peer Discovery Deduplication Fix - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-23
**Phase:** 51-peer-discovery-deduplication
**Areas discussed:** Bug reproduction, Dedup strategy, Scope boundary

---

## Bug Reproduction

### UI Surface

| Option             | Description                                     | Selected |
| ------------------ | ----------------------------------------------- | -------- |
| Setup 配对发现页   | 在 Setup 流程中扫描附近设备时，同一设备出现多次 | ✓        |
| Devices 设备列表页 | 在主界面的已配对设备列表中看到重复              |          |
| PairingDialog      | 在配对对话框中扫描可配对设备时出现重复          |          |
| CLI peers 命令     | 通过 CLI 查询 peers 时返回重复条目              |          |

**User's choice:** Setup 配对发现页

### Duplication Pattern

| Option                 | Description                                         | Selected |
| ---------------------- | --------------------------------------------------- | -------- |
| 同一设备名出现 2-3 次  | 同一个设备名称在列表中重复出现，但 peer_id 可能不同 |          |
| 完全相同的条目出现多次 | 同一个 peer_id + 同一个设备名完全重复               |          |
| 不确定                 | 没仔细看过 peer_id，只知道看到重复的设备            |          |

**User's choice:** Other — 不同的设备名，不同的 peer ID（同一台物理机器）

### Root Cause Hypothesis

| Option          | Description                                                                 | Selected |
| --------------- | --------------------------------------------------------------------------- | -------- |
| Daemon 重启导致 | 同一台机器 daemon 重启后生成了新 peer_id，旧 peer_id 的 mDNS 记录还在缓存中 |          |
| 多网卡/多地址   | 同一台机器有多个网络接口，每个接口被识别为独立设备                          |          |
| 不确定根因      | 只知道同一台机器出现多次，不确定具体原因                                    | ✓        |

**User's choice:** 不确定根因

### Environment

| Option             | Description                                         | Selected |
| ------------------ | --------------------------------------------------- | -------- |
| 开发环境 dual mode | 用 tauri:dev:dual 或 peerA/peerB 双终端开发时观察到 | ✓        |
| 生产环境多设备     | 实际多台物理设备之间配对时观察到                    |          |
| 两者都有           | 开发和生产环境都见过这个问题                        |          |

**User's choice:** 开发环境 dual mode

### Scenario Details

| Option         | Description                                          | Selected |
| -------------- | ---------------------------------------------------- | -------- |
| peerA 出现多次 | 同一个 peerA 在列表中出现 2-3 条                     |          |
| 幽灵设备       | 除了真实的 peerA，还出现了之前 daemon 重启前的旧记录 |          |
| 不确定         | 没有足够信息判断                                     |          |

**User's choice:** Other — peerA 出现了一次，还有一个未知设备

### Unknown Device

| Option         | Description                                                  | Selected |
| -------------- | ------------------------------------------------------------ | -------- |
| 可能是自发现   | peerB 的 daemon 通过 mDNS 发现了自己，应该过滤掉本机 peer_id | ✓        |
| 是其他网络设备 | 可能是局域网上其他运行 UniClipboard 的设备                   |          |
| 不确定         | 没有足够信息判断                                             |          |

**User's choice:** 可能是自发现

---

## Dedup Strategy

| Option                     | Description                                                                                    | Selected |
| -------------------------- | ---------------------------------------------------------------------------------------------- | -------- |
| 后端单层治理 (Recommended) | 在 daemon/app 层根治——找出为什么同一台机器会产生多个 peer_id，从源头解决。前端不需要额外处理。 | ✓        |
| 后端治理 + 前端安全网      | 后端修复根因，同时前端加一层基于 device_id 或 device_name 的去重保护，防御性编程。             |          |
| 仅前端去重                 | 不改后端，前端按 device_id 或其他唯一标识去重显示。快速但没有解决根因。                        |          |

**User's choice:** 后端单层治理 (Recommended)

---

## Scope Boundary

| Option                             | Description                                                                   | Selected |
| ---------------------------------- | ----------------------------------------------------------------------------- | -------- |
| 仅修复 mDNS 重复问题 (Recommended) | 专注解决同一物理机器出现多个 peer_id 的问题。消除根因 + 验证修复。            | ✓        |
| 修复 + 过期 peer 清理              | 除了去重，还处理 mDNS 过期后 peer 残留问题，确保断开连接的设备不会永久显示。  |          |
| 修复 + peer 身份稳定性             | 确保 daemon 重启后 peer_id 稳定不变（持久化 keypair），从根本上消除重复来源。 |          |

**User's choice:** 仅修复 mDNS 重复问题 (Recommended)

---

## Claude's Discretion

- 具体根因定位由 researcher 调查（GUI 双 libp2p 实例、dual mode identity 冲突、snapshot 聚合问题等）
- 修复方案的技术选型

## Deferred Ideas

- 过期 peer 清理
- peer_id 持久化稳定性
- 前端防御性去重
