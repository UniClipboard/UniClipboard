-- The manifest is rebuildable; older binaries do not consume root names.
DELETE FROM entry_file_set;

ALTER TABLE entry_file_set DROP COLUMN root_name_ct;
