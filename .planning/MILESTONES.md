# Milestones

## v0.4.0 Runtime Mode Separation (Shipped: 2026-04-09)

**Phases completed:** 52 phases, 129 plans, 224 tasks

### Delivered

The app finished the long split from a Tauri-centered runtime into a shared GUI / CLI / daemon shape. During this milestone, the shared startup path was extracted, the daemon and CLI were built out into real products, pairing and setup moved onto daemon-owned paths, clipboard sync and restore were pushed deeper into the daemon side, and the old local tracing path was replaced by the new OTLP-based observability path.

### Accomplishments

1. Shared runtime and startup path were established, so GUI, CLI, and daemon now build from the same core assembly instead of each owning separate business wiring.
2. Daemon and CLI became usable runtime modes with startup, status, clipboard commands, lifecycle management, and release packaging.
3. Pairing, setup, and auth were pulled toward daemon-owned flows, reducing duplicated app-side logic and unifying the command path.
4. Clipboard capture, inbound sync, outbound sync, restore, and write-boundary handling were consolidated around daemon-side execution and a single write coordinator.
5. Frontend pairing/event handling was simplified and the old p2p facade was removed.
6. Observability moved to OTLP with cross-device trace propagation and archived Seq support materials updated for the new pipeline.

### Known Gaps

- This milestone was archived with gaps accepted, not with a clean pass.
- Planning files still had bookkeeping drift at archive time: some requirement rows, verification files, and milestone scope records did not fully agree.
- GUI-launched daemon processes still did not automatically inherit the OTLP endpoint, so that path was not fully closed.
- Some verification work was still missing, stale, or left as manual follow-up.

### Archive Files

- `.planning/milestones/v0.4.0-ROADMAP.md`
- `.planning/milestones/v0.4.0-REQUIREMENTS.md`
- `.planning/milestones/v0.4.0-MILESTONE-AUDIT.md`
- `.planning/milestones/v0.4.0-phases/`

---
---

## v0.3.0 — Log Observability & Feature Expansion

**Shipped:** 2026-03-17
**Phases:** 19 (19-35) | **Plans:** 51 | **Tasks:** ~95
**Timeline:** 8 days (2026-03-09 → 2026-03-17)
**Files changed:** 594 (+67,065 / -4,804)
**Commits:** 363

### Delivered

Full observability pipeline from structured dual-output logging through flow correlation, Seq integration, and cross-device distributed tracing. Content type sync filtering with per-device and global master toggle. File sync foundation through chunked libp2p transfer, clipboard integration, UI, settings, quota enforcement, and eventual consistency. Link content type support, macOS keychain auto-unlock modal, event-driven device discovery, and consolidated outbound sync planner.

### Accomplishments

1. Dual-output tracing (pretty console + JSON) with 3 configurable log profiles (dev/prod/debug_clipboard) via uc-observability crate.
2. Flow correlation with `flow_id` and `stage` spans across all clipboard capture and sync stages, including cross-spawn and cross-layer propagation.
3. Seq local integration with CLEF format, async batching, device_id injection, and docker-compose for LAN cross-device tracing.
4. Per-device sync settings with content type toggles (text/image/link/file), global master toggle, cascade disable UX, and OutboundSyncPlanner consolidation.
5. Complete file sync pipeline: libp2p chunked transfer with Blake3 verification, serial queue, retry logic, Dashboard UI with progress/notifications, clipboard integration, quota enforcement, auto-cleanup, and eventual consistency with durable transfer lifecycle tracking.
6. Link content type (MIME + URL detection), macOS keychain modal, event-driven device discovery replacing polling, and keyboard shortcuts settings UI.

### Known Gaps

- KB-04 to KB-07: Code implemented (commit a31239c9) but Phase 27 missing VERIFICATION.md and 27-02-SUMMARY.md. REQUIREMENTS.md checkboxes were never updated.
- CT-06 requirement text stale: says file/link toggles should show "Coming Soon" but both are now interactive (correct behavior post Phase 28/31/32).
- REQUIREMENTS.md traceability table was stale for 25+ entries at time of archive.
- SUMMARY frontmatter `requirements-completed` field only populated for Phase 26 (2/49 files).
- Phase 35: Global auto_sync guard not consolidated into OutboundSyncPlanner (two enforcement points, by design).
- Windows disk space check uses optimistic fallback in file sync quota enforcement.
- Nyquist validation partial: 13/19 phases compliant, 6 missing VALIDATION.md.

### Git

- Range: `646be4a5..a6b1d766`
- Tag: `v0.3.0`

---

## v0.2.0 — Architecture Remediation

**Shipped:** 2026-03-09
**Phases:** 9 (10-18) | **Plans:** 22 | **Tasks:** ~34
**Timeline:** 4 days (2026-03-06 → 2026-03-09)
**LOC:** 115,362 Rust + 17,530 TypeScript

### Delivered

Remediating architecture defects from issue #214: enforced hexagonal boundary contracts with compiler-verified private deps, established typed CommandError/DTO command surfaces, added lifecycle governance with TaskRegistry and graceful shutdown, decomposed god-object orchestrators, wired lifecycle/clipboard management commands E2E, replaced dashboard full-reload with incremental prepend, and migrated theme engine to runtime TS preset injection.

### Accomplishments

1. Compiler-enforced boundary contracts: private `deps` field, facade accessors, TransferPayloadDecryptorPort injection, non-domain port eviction from uc-core.
2. CommandError enum with 6 typed variants replacing all `Result<T, String>` command returns, LifecycleStatusDto/PairedPeer DTOs.
3. TaskRegistry with CancellationToken cascade, graceful shutdown on app exit, StagedPairedDeviceStore injection.
4. Orchestrator decomposition: SetupActionExecutor, PairingProtocolHandler/SessionManager, AppDeps sub-structs, 12 shared noop test helpers.
5. Dashboard optimized from 330 → 63 lines via useClipboardEvents hook with origin-based routing (local=prepend, remote=throttled reload).
6. Runtime theme preset engine with TS token maps, SettingProvider CSS variable injection, multi-dot Appearance swatches.
7. Chunked transfer backend (256KB network I/O with TransferProgressPort) — partial, frontend deferred.

### Known Gaps

- Phase 18 (Chunked Transfer Resume) incomplete: 1/3 plans executed, CT-02/CT-04/CT-05 unsatisfied.
- Transfer progress frontend phantom code: SUMMARY claims completion but files not on branch.
- BOUND-03/BOUND-04 documentation drift: verified as SATISFIED but REQUIREMENTS.md checkboxes not updated at time of archive.
- Lifecycle events logged only, not emitted as `lifecycle://event` (frontend polls instead of event-driven).
- `is_favorited` field hard-coded to false (domain model lacks favorites column).
- `testing.rs` noops only adopted by 1 test file.
- `clear_clipboard_items` backend command not registered (pre-existing).

### Git

- Range: milestone work on branch `mkdir700/stockholm-v1`
- Tag: `v0.2.0`

---

## v0.1.0 — Daily Driver

**Shipped:** 2026-03-06
**Phases:** 9 | **Plans:** 17 | **Tasks:** ~30
**Timeline:** 5 days (2026-03-02 → 2026-03-06)
**Files changed:** 204 (+23,577 / -1,703)

### Delivered

Evolved UniClipboard from baseline LAN clipboard sync into a production-ready daily driver with unified encrypted transfer, true streaming inbound decode, optimized at-rest blob format, reliable Windows image capture, cross-platform dashboard image rendering, setup-flow UX redesign, V3 binary sync protocol with zero-copy fanout, and large-image clipboard read/memory optimizations.

### Accomplishments

1. Unified transfer protocol (UTL-01..07) with per-chunk XChaCha20-Poly1305 encryption and V1 compatibility.
2. Two-segment wire framing + transport-level streaming decode to remove `read_to_end` bottleneck.
3. Binary V2 blob-at-rest format + zstd compression + migration and spool cleanup flow.
4. Windows image capture stabilized via clipboard-rs upgrade and native CF_DIB fallback.
5. V3 binary sync pipeline delivered (compression + Arc zero-copy fanout + V3-only inbound rewrite).
6. macOS large-image read pipeline optimized (TIFF alias dedup + deferred conversion), reducing peak memory.

### Known Gaps

- Milestone audit status for original v0.1 scope was `tech_debt` (not strict `passed`).
- Phase 01: Missing formal VERIFICATION.md (informal verification recorded in SUMMARY).
- Pre-existing test failure: `business_command_timeouts_cover_stream_operation_budgets`.
- Human verification pending: multi-device memory profile and E2E inbound with active encryption session.
- `broadcast_clipboard` port remains unimplemented (pre-existing).
- `.planning/REQUIREMENTS.md` was not maintained as active file during phases 4-9; requirements were reconstructed from roadmap/summaries.

### Git

- Range: `8304405..bab0ae7`
- Tag: `v0.1.0`
