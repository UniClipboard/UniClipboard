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

### Phase 4: Optimize blob at-rest storage format without backward compatibility

**Goal:** Replace JSON-serialized EncryptedBlob at-rest format with a compact binary format (29-byte header + raw ciphertext), add zstd compression before encryption, track compressed_size in DB, and wipe old blobs on upgrade.
**Requirements:** [BLOB-01, BLOB-02, BLOB-03, BLOB-04]
**Depends on:** Phase 3
**Plans:** 2 plans

Plans:

- [ ] 04-01-PLAN.md — Contracts, domain models, schema migration (AAD v2, BlobStorePort, Diesel)
- [ ] 04-02-PLAN.md — V2 binary format implementation with zstd compression + spool cleanup
