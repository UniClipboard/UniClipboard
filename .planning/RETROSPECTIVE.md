# Project Retrospective

_A living document updated after each milestone. Lessons feed forward into future planning._

## Milestone: v0.2.0 — Architecture Remediation

**Shipped:** 2026-03-09
**Phases:** 9 | **Plans:** 22

### What Was Built

- Compiler-enforced hexagonal boundary contracts (private deps, facade accessors, port injection)
- Typed CommandError enum (6 variants) and DTO command surfaces across all Tauri commands
- TaskRegistry with CancellationToken cascade for deterministic lifecycle governance
- Orchestrator decomposition (Setup/Pairing) with shared noop test infrastructure
- Lifecycle DTO and clipboard management command wiring with frontend integration
- Dashboard refresh optimization (330 → 63 lines, incremental prepend + throttled remote reload)
- Runtime theme preset engine with TS token maps and multi-dot Appearance swatches
- Chunked transfer backend (256KB I/O with progress events) — partial completion

### What Worked

- Phase dependency chains (10→11→12→13→14/15→16→17→18) enabled incremental delivery with stable foundations
- VERIFICATION.md at each phase caught documentation drift early (BOUND-03/BOUND-04 checkbox discrepancy)
- Integration checker at audit time identified phantom code (17-chunk-03) before it became a bigger problem
- Fast plan execution: most plans completed in 5-15 minutes (architecture changes are well-scoped when requirements are clear)
- Quick task workflow (gsd:quick) effectively handled the TransferProgressBar removal mid-milestone

### What Was Inefficient

- Phase 18 directory naming mismatch (17-chunk-transfer-resume vs roadmap Phase 18) caused tooling confusion
- Phase 18 plan 03 SUMMARY was written but code never landed on branch — phantom completion
- REQUIREMENTS.md documentation wasn't updated when BOUND-03/BOUND-04 were verified as complete
- Phases 16-18 requirements (P16-_, P17-_, CT-\*) defined only in ROADMAP, not REQUIREMENTS.md — harder to audit
- Nyquist validation was never achieved (all phases partial) — wave_0 tests not generated

### Patterns Established

- `runtime.usecases()` accessor pattern as the only command → use case bridge
- `CommandError` serde `tag=code content=message` for frontend discriminated unions
- `TaskRegistry.spawn()` for all long-lived async tasks with CancellationToken cascade
- AppDeps domain sub-structs (ClipboardPorts, SecurityPorts, DevicePorts, StoragePorts, SystemPorts)
- `testing.rs` as shared noop module (pub, not cfg(test)) for integration test access
- Origin-aware clipboard events with local=prepend/remote=throttle routing in useClipboardEvents hook
- Runtime theme preset registry with TS token maps injected via SettingProvider

### Key Lessons

1. Always update REQUIREMENTS.md checkboxes at the same time as VERIFICATION.md — documentation drift compounds
2. Define ALL milestone requirements in REQUIREMENTS.md (not just ROADMAP success criteria) for clean audit traceability
3. Phantom code detection: verify SUMMARY claims against actual branch state during phase verification
4. Phase numbering in directories must match roadmap phase numbers to avoid tooling gaps
5. Quick tasks (`gsd:quick`) are effective for small mid-milestone corrections

### Cost Observations

- Model mix: ~60% opus, ~30% sonnet, ~10% haiku (estimated)
- Notable: 22 plans across 9 phases in 4 days — architecture remediation was well-scoped by issue #214

---

## Milestone: v0.1.0 — Daily Driver

**Shipped:** 2026-03-03
**Phases:** 3 | **Plans:** 6

### What Was Built

- Download progress bar for Tauri update dialogs (IPC Channel)
- V2 chunked transfer protocol (XChaCha20-Poly1305 per-chunk AEAD, blake3 nonces)
- Multi-representation clipboard sync with priority selection (image > html > rtf > plain)
- Two-segment wire framing eliminating 33% base64 overhead
- True inbound streaming via SyncIoBridge + spawn_blocking (~1x chunk memory)
- V1 backward compatibility for old devices

### What Worked

- TDD approach for crypto engine (9 tests for ChunkedEncoder/Decoder) caught correctness issues early
- Plan dependency chain (02-01 → 02-02 → 02-03 → 03-01 → 03-02) enabled incremental, low-risk delivery
- serde_with Base64 discovery during 02-01 avoided wire format issues downstream
- Phase 03 (tech debt) immediately followed Phase 02 — resolved read_to_end while context was fresh
- Fast execution: ~88 minutes total for 6 plans across 3 phases

### What Was Inefficient

- Phase 01 was shipped before GSD workflow — no formal VERIFICATION.md
- CLI `milestone complete` tool only detected 1 SUMMARY (Phase 01) — had to do manual archival
- No standalone REQUIREMENTS.md file — requirements tracked inline in ROADMAP.md, making cross-referencing harder during audit

### Patterns Established

- Binary fields in network protocol messages use `serde_with` Base64 for compact JSON encoding
- New enum payload versions use `serde(default)` for backward compatibility
- Chunk AAD is binary concatenation (transfer_id || chunk_index_LE)
- Version dispatch: check payload_version before decryption, route to separate handler
- Tamper-resilient V2 decode: log error, return Ok(Skipped), never propagate decode errors
- Two-segment wire framing: [4-byte LE len][JSON header][optional raw trailing payload]
- Transport-level streaming decode via SyncIoBridge + spawn_blocking for bridging async → sync IO

### Key Lessons

1. Always create a standalone REQUIREMENTS.md at milestone start — inline tracking in ROADMAP.md is harder to audit
2. serde_bytes does NOT produce base64 in JSON — always verify serialization format assumptions with tests
3. Tech debt phases work best immediately after the phase that created the debt — context is still warm
4. The ProcessedMessage enum pattern cleanly separates protocol dispatch from business logic
5. Pre-decoded plaintext fast paths with fallback decode provide both performance and robustness

### Cost Observations

- Model mix: ~70% opus, ~20% sonnet, ~10% haiku (estimated)
- Notable: 6 plans in ~88 minutes — chunked transfer domain was well-scoped

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Phases | Plans | Key Change                                                           |
| --------- | ------ | ----- | -------------------------------------------------------------------- |
| v0.1.0    | 3      | 6     | First milestone with GSD workflow (Phase 01 predates it)             |
| v0.2.0    | 9      | 22    | Full GSD workflow with verification, audit, and integration checking |

### Cumulative Quality

| Milestone | Tests Added | VERIFICATION Score   | Known Gaps                       |
| --------- | ----------- | -------------------- | -------------------------------- |
| v0.1.0    | ~30+        | 30/30 (Phase 02+03)  | 5 tech debt items                |
| v0.2.0    | ~40+        | 59/59 (Phases 10-17) | 7 items (3 critical in Phase 18) |

### Top Lessons (Verified Across Milestones)

1. TDD for crypto code catches correctness issues that manual testing misses
2. Incremental plan chains reduce risk and maintain context continuity
3. Always maintain REQUIREMENTS.md as the single source of truth — inline/roadmap-only tracking creates audit gaps
4. Phase verification (VERIFICATION.md) should immediately update REQUIREMENTS.md checkboxes
5. Quick task workflow effectively handles small mid-milestone corrections without disrupting phase flow
