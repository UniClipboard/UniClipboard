-- Revert 2026-06-28-000001_add_search_document_char_count.

ALTER TABLE search_document DROP COLUMN char_count;
