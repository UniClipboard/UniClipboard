-- Revert 2026-06-25-000002_create_search_entry_tag.

DROP INDEX IF EXISTS idx_entry_tag_by_tag;
DROP TABLE IF EXISTS search_entry_tag;
