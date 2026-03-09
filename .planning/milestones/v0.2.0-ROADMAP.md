# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** - Phases 1-9 (shipped 2026-03-06)
- 🚧 **v0.2.0 Architecture Remediation** - Phases 10-13 (in progress)

## Phases

<details>
<summary>✅ v0.1.0 Daily Driver (Phases 1-9) - SHIPPED 2026-03-06</summary>

See: `.planning/milestones/v0.1.0-ROADMAP.md`

</details>

### 🚧 v0.2.0 Architecture Remediation (In Progress)

**Milestone Goal:** Eliminate root-cause architecture defects from issue #214 while preserving daily-driver clipboard sync behavior.

#### Phase 10: Boundary Repair Baseline

**Goal**: Restore strict dependency direction and close command-layer penetration paths.
**Depends on**: Phase 9 (milestone baseline)
**Requirements**: BOUND-01, BOUND-02, BOUND-03, BOUND-04
**Success Criteria** (what must be TRUE):

1. Commands perform business operations through use cases rather than direct runtime deps access.
2. Runtime dependency containers are no longer externally exposed to command modules.
3. Platform decode path uses core-defined port contracts instead of cross-adapter crate coupling.
4. Non-domain ports are removed from core and referenced through proper outer-layer placement.
   **Plans**: 3 plans

Plans:

- [ ] 10-01: Lock down runtime/usecase access boundaries and remove command bypasses
- [ ] 10-02: Introduce/route transfer decode abstraction through core port contracts
- [ ] 10-03: Move non-domain ports out of core and update wiring/tests

#### Phase 11: Command Contract Hardening

**Goal**: Establish stable DTO and typed error contracts for command surfaces.
**Depends on**: Phase 10
**Requirements**: CONTRACT-01, CONTRACT-02, CONTRACT-03, CONTRACT-04
**Success Criteria** (what must be TRUE):

1. Command endpoints return DTOs instead of leaking domain models.
2. Command failures use structured typed error categories, not generic `String` responses.
3. Event/command payload serialization is frontend-compatible and verified by tests.
4. Timeout, cancellation, and internal failures are distinguishable at command boundary.
   **Plans**: 2 plans

Plans:

- [ ] 11-01: Implement command DTO mapping layer and payload contract tests
- [ ] 11-02: Introduce typed command error taxonomy and migrate command handlers

#### Phase 12: Lifecycle Governance Baseline

**Goal**: Make async task lifecycle deterministic through cancellation and graceful shutdown governance.
**Depends on**: Phase 11
**Requirements**: LIFE-01, LIFE-02, LIFE-03, LIFE-04
**Success Criteria** (what must be TRUE):

1. App close/restart does not leave orphaned sync/pairing tasks.
2. Spawned workers are tracked and shutdown with bounded cancellation + join behavior.
3. Staging/session state is lifecycle-owned and no longer managed by unsafe globals.
4. Encryption/session behavior has one authoritative implementation path.
   **Plans**: 2 plans

Plans:

- [ ] 12-01: Introduce task ownership/cancellation framework and runtime shutdown flow
- [ ] 12-02: Remove unmanaged global state and unify encryption/session ownership path

#### Phase 13: Responsibility Decomposition & Testability

**Goal**: Reduce god-object complexity and improve maintainability/test velocity with guarded refactors.
**Depends on**: Phase 12
**Requirements**: DECOMP-01, DECOMP-02, DECOMP-03, DECOMP-04
**Success Criteria** (what must be TRUE):

1. High-risk sync/setup use-case modules are split into focused components with clear responsibilities.
2. Dependency organization is grouped to reduce broad god-container coupling.
3. Shared test helpers/noops reduce duplicate scaffold code and setup overhead.
4. Regression checks confirm pairing, sync, and setup flows remain stable post-decomposition.
   **Plans**: 3 plans

Plans:

- [ ] 13-01-PLAN.md — Shared test helpers + AppDeps domain sub-grouping
- [ ] 13-02-PLAN.md — Setup orchestrator action executor extraction
- [ ] 13-03-PLAN.md — Pairing orchestrator protocol handler + session manager extraction

#### Phase 14: Lifecycle DTO Frontend Integration

**Goal**: Align frontend lifecycle APIs with backend LifecycleStatusDto command contract and restore lifecycle status UI.
**Depends on**: Phase 13
**Requirements**: CONTRACT-01, CONTRACT-03
**Gap Closure**: Closes lifecycle DTO mismatch between frontend and `get_lifecycle_status` command and fixes the broken lifecycle status display flow.
**Plans**: 0 plans

#### Phase 15: Clipboard Management Command Wiring

**Goal**: Provide backend commands for clipboard stats/item/favorite management and wire them to existing frontend APIs.
**Depends on**: Phase 13
**Requirements**: CONTRACT-03
**Gap Closure**: Closes missing clipboard stats/item/favorite commands and restores clipboard management flow from frontend to uc-tauri.
**Plans**: 0 plans

## Progress

| Phase                                          | Milestone | Plans Complete | Status     | Completed |
| ---------------------------------------------- | --------- | -------------- | ---------- | --------- |
| 10. Boundary Repair Baseline                   | 3/3       | Complete       | 2026-03-06 | -         |
| 11. Command Contract Hardening                 | 2/2       | Complete       | 2026-03-06 | -         |
| 12. Lifecycle Governance Baseline              | 2/2       | Complete       | 2026-03-06 | -         |
| 13. Responsibility Decomposition & Testability | 3/3       | Complete       | 2026-03-06 | -         |

### Phase 16: Optimize DashboardPage refresh mechanism on new clipboard content

**Goal:** Replace full-reload pattern with incremental updates for local captures and throttled full-reload for remote sync, extracting event/state management into a dedicated useClipboardEvents hook.
**Requirements**: P16-01, P16-02, P16-03, P16-04, P16-05, P16-06
**Depends on:** Phase 15
**Plans:** 2/2 plans complete

Plans:

- [ ] 16-01-PLAN.md — Backend origin event + get_clipboard_entry command + Redux reducers
- [ ] 16-02-PLAN.md — useClipboardEvents hook extraction + DashboardPage simplification

### Phase 17: Dynamic theme switching with shadcn preset import and enhanced color preview

**Goal:** Migrate theme color ownership to runtime TS preset injection and upgrade Appearance preset swatches to multi-dot previews while preserving mode/persistence behavior.
**Requirements**: P17-01, P17-02, P17-03, P17-04, P17-05, P17-06, P17-07
**Depends on:** Phase 16
**Plans:** 2/2 plans complete

Plans:

- [ ] 17-01-PLAN.md — Runtime preset engine + SettingProvider injection + static CSS theme migration
- [ ] 17-02-PLAN.md — Appearance multi-dot swatches + persistence contract regression tests

### Phase 18: Chunked Transfer and Resume

**Goal:** Implement chunked network I/O, transfer resume capability, and progress feedback for the clipboard sync pipeline so that large payloads transfer reliably with user-visible progress.
**Requirements**: CT-01, CT-02, CT-03, CT-04, CT-05
**Depends on:** Phase 17
**Success Criteria** (what must be TRUE):

1. Outbound clipboard transfers write encrypted payload to the network in 256KB chunks instead of a single monolithic write_all (CT-01).
2. Inbound clipboard reads decode V3 chunks individually with progress tracking instead of buffering entire stream with read_to_end (CT-02).
3. Transfer progress events are emitted to the frontend at a throttled rate during active transfers (CT-03).
4. Interrupted transfers of large payloads (>1MB) can resume from the last successful chunk using in-memory TTL cache (CT-04).
5. The DashboardPage displays real-time transfer progress with direction, peer, and completion percentage (CT-05).

**Plans:** 1/3 plans executed

Plans:

- [ ] 18-01-PLAN.md — Chunked network I/O + TransferProgressPort + Tauri progress events
- [ ] 18-02-PLAN.md — Transfer resume protocol (ProtocolMessage::TransferResume + sender cache + receiver recovery)
- [ ] 18-03-PLAN.md — Frontend transfer progress UI (Redux slice + TransferProgressBar component)
