# Roadmap: UniClipboard Desktop

## Milestones

- 🚧 **v0.1.0 Daily Driver** - Phases 01-02+ (in progress)

## Phases

<details>
<summary>✅ Phase 1: Add download progress display - SHIPPED 2026-03-03</summary>

### Phase 1: Add download progress display

**Goal**: Real-time download progress bar in update dialogs via Tauri IPC Channel
**Plans**: 1 plan

Plans:

- [x] 01-01: Bridge download callbacks to React progress bar

</details>

### 🚧 v0.1.0 Daily Driver (In Progress)

**Milestone Goal:** Evolve from basic clipboard sync into a daily-driver productivity tool with quick-paste, image support, and robust data transfer.

## Progress

| Phase                        | Milestone | Plans Complete | Status   | Completed  |
| ---------------------------- | --------- | -------------- | -------- | ---------- |
| 1. Download progress display | v0.1.0    | 1/1            | Complete | 2026-03-03 |

### Phase 2: 实现统一数据传输层：不关心内容类型（文本/图片/文件），内部自动分块，对方设备校验拼装后写入剪切板

**Goal:** Replace V1 text-only clipboard sync with a unified chunked transfer layer: all clipboard representations (text/image) bundled, chunk-level XChaCha20-Poly1305 encrypted (deterministic nonces via blake3), transferred over existing libp2p transport, receiver validates, reassembles, and writes highest-priority representation to clipboard. No UI changes — content appears silently.
**Requirements**: UTL-01, UTL-02, UTL-03, UTL-04, UTL-05, UTL-06, UTL-07
**Depends on:** Phase 1
**Plans:** 3 plans

Plans:

- [ ] 02-01-PLAN.md — V2 protocol type contracts in uc-core (ClipboardMessage v2 field, ClipboardMultiRepPayloadV2, chunk AAD helper)
- [ ] 02-02-PLAN.md — ChunkedEncoder/Decoder in uc-infra (XChaCha20-Poly1305 per-chunk AEAD, binary wire format)
- [ ] 02-03-PLAN.md — Wire V2 into sync_outbound + sync_inbound use cases; raise transport limits

---
