# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** — Phases 1-3 (shipped 2026-03-03)

## Phases

<details>
<summary>✅ v0.1.0 Daily Driver (Phases 1-3) — SHIPPED 2026-03-03</summary>

- [x] Phase 1: Download progress display (1/1 plan) — completed 2026-03-03
- [x] Phase 2: Unified transfer layer (3/3 plans) — completed 2026-03-03
- [x] Phase 3: True inbound streaming (2/2 plans) — completed 2026-03-03

See: `.planning/milestones/v0.1-ROADMAP.md` for full details.

</details>

## Progress

| Phase                        | Milestone | Plans Complete | Status   | Completed  |
| ---------------------------- | --------- | -------------- | -------- | ---------- |
| 1. Download progress display | v0.1.0    | 1/1            | Complete | 2026-03-03 |
| 2. Unified transfer layer    | v0.1.0    | 3/3            | Complete | 2026-03-03 |
| 3. True inbound streaming    | v0.1.0    | 2/2            | Complete | 2026-03-03 |
| 4. Blob at-rest storage opt  | v0.1      | 2/2            | Complete | 2026-03-04 |
| 5. Windows image capture     | v0.1      | 2/2            | Complete | 2026-03-05 |
| 6. Dashboard image display   | v0.1      | 1/1            | Complete | 2026-03-05 |

### Phase 4: Optimize blob at-rest storage format without backward compatibility

**Goal:** Replace JSON-serialized EncryptedBlob at-rest format with a compact binary format (29-byte header + raw ciphertext), add zstd compression before encryption, track compressed_size in DB, and wipe old blobs on upgrade.
**Requirements:** [BLOB-01, BLOB-02, BLOB-03, BLOB-04]
**Depends on:** Phase 3
**Plans:** 2 plans

Plans:

- [x] 04-01-PLAN.md — Contracts, domain models, schema migration (AAD v2, BlobStorePort, Diesel)
- [x] 04-02-PLAN.md — V2 binary format implementation with zstd compression + spool cleanup

### Phase 5: Fix Windows clipboard image capture

**Goal:** Make clipboard image capture work reliably on Windows by upgrading clipboard-rs to 0.3.3 and adding a native CF_DIB fallback via clipboard-win when clipboard-rs fails to read images. Screenshots (Win+Shift+S, Print Screen), browser image copies, and image editor copies should all produce valid image/png representations.
**Requirements:** [WIN-IMG-01, WIN-IMG-02, WIN-IMG-03, WIN-IMG-04, WIN-IMG-05, WIN-IMG-06]
**Depends on:** Phase 4
**Plans:** 2 plans

Plans:

- [x] 05-01-PLAN.md — Upgrade clipboard-rs, fix BMP-to-PNG conversion, add unit tests
- [x] 05-02-PLAN.md — Wire Windows-native image fallback into read_snapshot + manual verification

### Phase 6: Fix dashboard image display

**Goal:** Fix cross-platform image display in the dashboard by using Tauri's convertFileSrc API to generate platform-correct URLs for the uc:// custom protocol, replacing raw uc:// URLs that fail on Windows (WebView2 requires http://uc.localhost/ format).
**Requirements:** [IMG-DISPLAY-01, IMG-DISPLAY-02]
**Depends on:** Phase 5
**Plans:** 1/1 plans complete

Plans:

- [x] 06-01-PLAN.md — Create resolveUcUrl helper with convertFileSrc, wire into ClipboardItem + API layer

### Phase 7: Redesign setup flow UX for cross-platform consistency

**Goal:** Redesign the setup flow frontend (SetupPage + all step components) to achieve consistent UX across Windows, macOS, and Linux. Extract a shared StepLayout component, unify slide animations with directional transitions, change WelcomeStep to vertical card layout, add step dot indicators, and standardize on sm: breakpoint only.
**Requirements:** [UX-01, UX-02, UX-03, UX-04, UX-05, UX-06, UX-07, UX-08]
**Depends on:** Phase 6
**Plans:** 1/2 plans executed

Plans:

- [ ] 07-01-PLAN.md — Create StepLayout, StepDotIndicator, ProcessingJoinStep components and types
- [ ] 07-02-PLAN.md — Migrate all steps to StepLayout, update SetupPage orchestrator, visual verification

### Phase 8: Optimize large image sync pipeline (V3 binary protocol, compression, zero-copy fanout)

**Goal:** Replace V2 JSON+base64 clipboard sync protocol with V3 binary wire format (37-byte header, length-prefixed payload codec), add zstd compression before encryption inside chunked transfer, eliminate per-peer memory copies via Arc<[u8]> zero-copy fanout, parallelize encrypt+ensure_business_path with tokio::join!, and delete all V1/V2 legacy code paths.
**Requirements:** [V3-CODEC, V3-WIRE, V3-COMPRESS, V3-LARGE, V3-ARC, V3-OUTBOUND, V3-INBOUND, V3-NOENC, V3-NOLEAK]
**Depends on:** Phase 7
**Plans:** 3 plans

Plans:

- [ ] 08-01-PLAN.md — V3 binary payload codec (uc-core) + V3 chunked encoder/decoder with zstd compression (uc-infra)
- [ ] 08-02-PLAN.md — Port signature changes (Arc<[u8]>), outbound rewrite with V3 encode + parallelization, V1/V2 type deletion
- [ ] 08-03-PLAN.md — Inbound rewrite for V3-only decode, V2 chunked transfer removal, tracing spans
