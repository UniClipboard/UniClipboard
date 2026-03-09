# UniClipboard Desktop

## What This Is

A cross-platform clipboard synchronization app built with Tauri 2, React, and Rust. It provides encrypted LAN clipboard sync for text and images, with hexagonal architecture, typed command contracts, lifecycle governance, and optimized transfer/dashboard pipelines.

## Core Value

Seamless clipboard synchronization across devices — users can copy on one device and paste on another without interrupting their workflow.

## Current State

- **Latest shipped milestone:** v0.2.0 Architecture Remediation (2026-03-09)
- **Current capability level:** Daily-driver with hardened architecture, typed command surfaces, and lifecycle governance
- **Architecture status:** Hexagonal migration ~70% complete; boundary contracts compiler-enforced, command DTOs/errors typed, orchestrators decomposed
- **LOC:** 115,362 Rust + 17,530 TypeScript

## Requirements

### Validated

- ✓ Clipboard text capture and history — existing
- ✓ Device pairing and LAN sync baseline — existing
- ✓ V2 unified transfer and streaming decode foundation — v0.1.0
- ✓ At-rest blob format optimization and migration — v0.1.0
- ✓ Windows image clipboard capture reliability — v0.1.0
- ✓ Dashboard image display compatibility across platforms — v0.1.0
- ✓ Setup flow UX consistency improvements — v0.1.0
- ✓ V3 binary sync protocol, compression, and zero-copy fanout — v0.1.0
- ✓ Large-image clipboard read pipeline memory/latency improvements — v0.1.0
- ✓ Cross-layer boundary violation removal and command-layer penetration closure — v0.2.0
- ✓ Typed command DTO/error contracts and traceable API surfaces — v0.2.0
- ✓ Lifecycle governance (task cancellation, graceful shutdown, runtime cleanup) — v0.2.0
- ✓ God-object decomposition (AppDeps/SetupOrchestrator/PairingOrchestrator) — v0.2.0
- ✓ Test infrastructure consolidation (shared noop ports) — v0.2.0
- ✓ Dashboard incremental update with origin-based event routing — v0.2.0
- ✓ Runtime theme preset engine with multi-dot Appearance swatches — v0.2.0

### Active

- [ ] Complete chunked transfer resume protocol (CT-02, CT-04 — backend only, frontend deferred)
- [ ] Wire transfer progress events to frontend UI (CT-05)
- [ ] Add favorites persistence (domain model column needed)
- [ ] Wire lifecycle events to frontend (currently polling, not event-driven)
- [ ] Expand typed error migration to port surfaces (ARCHNEXT-01)
- [ ] Domain model refinement for anemic models (ARCHNEXT-02)

### Out of Scope

- WebDAV cross-internet sync — deferred
- File synchronization — deferred
- Mobile app — desktop-first
- OAuth/third-party login — not required for current product model

## Next Milestone Goals

- Complete chunked transfer and resume capability for reliable large-payload transfers.
- Wire transfer progress events to dashboard UI.
- Continue architecture hardening at port boundaries.
- Address favorites persistence and lifecycle event wiring.

## Context

Shipped v0.2.0 across phases 10-18 with major architecture remediation.
Tech stack remains Tauri 2 + React 18 + Rust + libp2p + XChaCha20-Poly1305.
Hexagonal boundaries now compiler-enforced; all Tauri commands use typed DTOs and CommandError.
Dashboard reduced from 330 to 63 lines with hook-based event management.

## Key Decisions

| Decision                                          | Rationale                                                    | Outcome |
| ------------------------------------------------- | ------------------------------------------------------------ | ------- |
| Two-segment framing for clipboard wire format     | Reduce overhead and enable stream decode                     | ✓ Good  |
| V3 binary protocol with Arc fanout                | Improve large payload performance and memory behavior        | ✓ Good  |
| Manual uc:// URL resolution strategy              | Ensure Windows/WebView compatibility                         | ✓ Good  |
| Background TIFF conversion                        | Keep clipboard capture path responsive                       | ✓ Good  |
| Private deps + facade accessors on AppRuntime     | Compiler-enforced boundary: commands cannot access internals | ✓ Good  |
| CommandError serde tag=code content=message       | Frontend discriminated union handling                        | ✓ Good  |
| TaskRegistry with CancellationToken cascade       | Deterministic shutdown without orphaned tasks                | ✓ Good  |
| StagedPairedDeviceStore as Arc-injected struct    | Replace OnceLock global with lifecycle-owned state           | ✓ Good  |
| AppDeps domain sub-structs (5 groups)             | Reduce god-container coupling                                | ✓ Good  |
| Origin-aware clipboard events                     | Enable local-prepend vs remote-throttle routing              | ✓ Good  |
| Runtime TS theme presets (not static CSS)         | Single source of truth, dynamic switching                    | ✓ Good  |
| Chunked 256KB network writes with progress events | Support large payload transfer with UX feedback              | ✓ Good  |

## Constraints

- **Tech stack:** Tauri 2 + React + Rust (fixed)
- **Sync domain:** LAN-first with libp2p
- **Security:** XChaCha20-Poly1305 remains mandatory
- **Platform support:** macOS primary; Windows/Linux supported

---

_Last updated: 2026-03-09 after v0.2.0 milestone completion_
