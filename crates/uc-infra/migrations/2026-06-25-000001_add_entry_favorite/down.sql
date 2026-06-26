-- Revert 2026-06-25-000001_add_entry_favorite.
--
-- DROP COLUMN requires SQLite 3.35+ (project runs 3.40+). is_favorited does not
-- participate in PK / UNIQUE / FK / INDEX, so it can be dropped directly.

ALTER TABLE clipboard_entry
DROP COLUMN is_favorited;
