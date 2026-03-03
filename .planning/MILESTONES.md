# Milestones

## v0.1.0 — Daily Driver

**Shipped:** 2026-03-03
**Phases:** 3 | **Plans:** 6 | **Tasks:** ~10
**Timeline:** 2 days (2026-03-02 → 2026-03-03)
**Files changed:** 81 (+10,161 / -700)

### Delivered

Evolved from basic text-only clipboard sync to a unified chunked transfer layer supporting multi-representation payloads (text/image) with per-chunk XChaCha20-Poly1305 encryption, two-segment wire framing, and true inbound streaming — all while maintaining V1 backward compatibility.

### Accomplishments

1. Real-time download progress bar in update dialogs via Tauri IPC Channel
2. V2 chunked transfer protocol with XChaCha20-Poly1305 per-chunk AEAD encryption and blake3 nonce derivation
3. Multi-representation clipboard sync (text/image) with priority-based selection (image > html > rtf > plain)
4. Two-segment wire framing eliminating ~33% base64 overhead for V2 payloads
5. True inbound streaming via SyncIoBridge + spawn_blocking, reducing peak memory from ~2x payload to ~1x chunk size
6. V1 backward compatibility maintained for old devices

### Known Gaps

- Phase 01: Missing formal VERIFICATION.md (informal verification in SUMMARY)
- Phase 03: Pre-existing test failure `business_command_timeouts_cover_stream_operation_budgets`
- Phase 03: Human verification pending — real-device V2 streaming memory profile
- Phase 03: Human verification pending — E2E V2 inbound with encryption session on two devices
- Pre-existing: `broadcast_clipboard` port unimplemented (sync_outbound uses per-peer loop)

### Git

- Range: `8304405..624291b`
- Tag: `v0.1.0`
