# Milestones

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
