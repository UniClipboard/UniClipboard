# Project Retrospective

_A living document updated after each milestone. Lessons feed forward into future planning._

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

| Milestone | Phases | Plans | Key Change                                               |
| --------- | ------ | ----- | -------------------------------------------------------- |
| v0.1.0    | 3      | 6     | First milestone with GSD workflow (Phase 01 predates it) |

### Cumulative Quality

| Milestone | Tests Added | VERIFICATION Score  | Known Gaps        |
| --------- | ----------- | ------------------- | ----------------- |
| v0.1.0    | ~30+        | 30/30 (Phase 02+03) | 5 tech debt items |

### Top Lessons (Verified Across Milestones)

1. TDD for crypto code catches correctness issues that manual testing misses
2. Incremental plan chains reduce risk and maintain context continuity
