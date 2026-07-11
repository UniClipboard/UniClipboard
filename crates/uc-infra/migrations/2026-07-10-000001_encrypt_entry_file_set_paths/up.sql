-- Encrypt the file-path columns of entry_file_set (ADR-010 phase 3 PR-A).
--
-- `original_text` stored device-local file paths (user content — filenames and
-- directory structure) as plaintext TEXT, leaking to disk against the
-- persist-as-ciphertext bedrock rule. Reseal it as an AEAD BLOB (UCFS
-- envelope). Also add the directory-member location columns that ADR-010 phase
-- 3 directory capture needs: root_index / relative_path (encrypted) / kind_tag.
-- These stay NULL until directory capture populates them in a later PR.
--
-- The manifest capture-write path only merged 2026-07-10, so existing rows are
-- negligible dev data and cannot be re-encrypted here (the MasterKey is
-- unavailable in a migration context). Drop them: the manifest rebuilds on the
-- next capture, and dispatch falls back to snapshot re-parse for any entry that
-- lacks one.
--
-- Requires SQLite >= 3.35 for ALTER TABLE ... DROP COLUMN. The dropped column
-- carries no index/view/trigger dependency (the composite PK covers entry_id).
DELETE FROM entry_file_set;

ALTER TABLE entry_file_set ADD COLUMN original_text_ct BLOB;
ALTER TABLE entry_file_set DROP COLUMN original_text;

ALTER TABLE entry_file_set ADD COLUMN root_index BIGINT;
ALTER TABLE entry_file_set ADD COLUMN relative_path_ct BLOB;
ALTER TABLE entry_file_set ADD COLUMN kind_tag TEXT;
