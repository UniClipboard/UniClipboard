-- Create blob_reference: plaintext-hash → ciphertext-digest dedup cache.
--
-- Slice 3 Phase 1. Consumed by D1 (PublishBlobUseCase) and D2 (FetchBlobUseCase)
-- skip-re-encrypt short-circuit path, and by T-03 (cross-device forwarding
-- sponsor-less) later as a prepopulation hook.
--
-- Columns map 1:1 to `uc-core::ports::blob::reference::{PlaintextHash, BlobDigest}`:
--   plaintext_hash := adapter-computed hash of plaintext bytes (primary key;
--                     stored as hex TEXT for sqlite-friendly dumps / CLI
--                     debugging — BLOB would be equivalent on disk)
--   digest         := adapter-computed hash of ciphertext bytes (content-
--                     addressed identity minted by the blob transfer adapter)
--   created_at     := unix seconds, project-wide timestamp convention (matches
--                     trusted_peer.trusted_at / peer_address.observed_at)
--
-- Upsert semantics: last-write-wins on plaintext_hash (port contract §3.2).
-- No space_id column — Phase 1 single-space assumption (see §3.2 key
-- decisions); multi-space goes through a future migration.

CREATE TABLE blob_reference (
    plaintext_hash TEXT PRIMARY KEY NOT NULL,
    digest         TEXT NOT NULL,
    created_at     INTEGER NOT NULL
);
