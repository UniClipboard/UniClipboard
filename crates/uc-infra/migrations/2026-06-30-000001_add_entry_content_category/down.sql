-- Revert 2026-06-30-000001_add_entry_content_category.

ALTER TABLE clipboard_entry
DROP COLUMN content_category;
