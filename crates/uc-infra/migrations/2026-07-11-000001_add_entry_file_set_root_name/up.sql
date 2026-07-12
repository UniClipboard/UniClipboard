-- Persist the selected top-level root basename needed by ADR-010 file-set-v1.
-- Root names are user content and therefore use the existing UCFS AEAD
-- envelope. Existing manifests are a rebuildable cache and predate root-name
-- modeling, so clear them rather than retain structurally incomplete rows.
--
-- This DELETE also covers an unrelated cipher hardening landing alongside
-- root-name support: `original_text_ct` / `relative_path_ct` now bind a
-- column-specific AAD suffix (previously they shared one AAD per row, which
-- let a ciphertext be transplanted between the two columns undetected).
-- Rows sealed before this change use the old AAD and cannot be decrypted
-- under the new scheme — clearing here means no row written before this
-- migration survives into a build that expects column-bound AAD.
DELETE FROM entry_file_set;

ALTER TABLE entry_file_set ADD COLUMN root_name_ct BLOB;
