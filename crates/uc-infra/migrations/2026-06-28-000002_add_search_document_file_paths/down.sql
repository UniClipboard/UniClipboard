-- Revert 2026-06-28-000002_add_search_document_file_paths.

ALTER TABLE search_document DROP COLUMN file_paths;
