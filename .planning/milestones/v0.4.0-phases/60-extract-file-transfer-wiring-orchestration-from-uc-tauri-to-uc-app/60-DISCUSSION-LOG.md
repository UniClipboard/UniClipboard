# Phase 60: Extract file transfer wiring orchestration from uc-tauri to uc-app - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-25
**Phase:** 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app
**Areas discussed:** Module organization, DTO ownership, wiring.rs integration

---

## Module Organization

| Option                          | Description                                                                                                                                                                             | Selected |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| FileTransferOrchestrator struct | Encapsulate as struct holding TrackInboundTransfersUseCase + HostEventEmitterPort, methods replace standalone functions. Consistent with SetupOrchestrator/PairingOrchestrator pattern. | ✓        |
| Standalone functions            | Move functions as-is to uc-app, no struct wrapper. Simpler, minimal changes.                                                                                                            |          |

**User's choice:** FileTransferOrchestrator struct
**Notes:** User preferred consistency with existing orchestrator patterns (SetupOrchestrator, PairingOrchestrator).

---

## DTO Ownership

| Option                       | Description                                                                                                                                                               | Selected |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Stay in uc-app (recommended) | FileTransferStatusPayload and EarlyCompletionCache as internal types of FileTransferOrchestrator module. HostEventEmitterPort already defines semantic events in uc-core. | ✓        |
| Promote to uc-core           | As shared wire DTO in uc-core::network. Suitable for cross-crate consumption but currently only used by file_transfer_wiring.                                             |          |

**User's choice:** Stay in uc-app
**Notes:** Only the orchestrator consumes these types; no cross-crate need.

---

## wiring.rs Integration

| Option                                | Description                                                                                                                                                 | Selected |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Assembly in assembly.rs (recommended) | assembly.rs creates FileTransferOrchestrator, wiring.rs receives via BackgroundRuntimeDeps. Consistent with CoreRuntime/SetupOrchestrator assembly pattern. | ✓        |
| In-place creation in wiring.rs        | wiring.rs constructs FileTransferOrchestrator directly. Simpler but assembly logic scattered.                                                               |          |
| Claude decides                        | Let Claude choose based on actual code structure.                                                                                                           |          |

**User's choice:** Assembly in assembly.rs
**Notes:** Consistent with established composition root pattern.

---

## Claude's Discretion

- Exact constructor signature and held deps
- spawn_timeout_sweep return type (JoinHandle vs TaskRegistry)
- Test placement strategy
- BackgroundRuntimeDeps field addition

## Deferred Ideas

None — discussion stayed within phase scope.
