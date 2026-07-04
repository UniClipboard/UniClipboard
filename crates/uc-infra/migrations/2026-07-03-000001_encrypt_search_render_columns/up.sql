-- Encrypt the content-derived render columns of search_document.
--
-- The five plaintext render columns (text_preview, file_names, link_urls,
-- file_paths, char_count) leaked clipboard content to disk, bypassing the v1
-- local-encrypted-search model. Replace them with a single AEAD-sealed BLOB
-- (UCSR envelope), backfilled by the search-v10 full rebuild.
--
-- source_device (SQL-filtered stable DeviceId) and payload_state (availability
-- flag) stay plaintext by explicit design decision.
--
-- Requires SQLite >= 3.35 for ALTER TABLE ... DROP COLUMN. The dropped columns
-- carry no index/view/trigger dependency.

ALTER TABLE search_document ADD COLUMN render_payload BLOB;

ALTER TABLE search_document DROP COLUMN text_preview;
ALTER TABLE search_document DROP COLUMN file_names;
ALTER TABLE search_document DROP COLUMN link_urls;
ALTER TABLE search_document DROP COLUMN file_paths;
ALTER TABLE search_document DROP COLUMN char_count;

-- Track the one-shot plaintext-residue purge (checkpoint + VACUUM) that runs
-- once after the search-v10 rebuild finalizes. NULL = not yet purged.
ALTER TABLE search_index_meta ADD COLUMN plaintext_purge_done_ms BIGINT;
