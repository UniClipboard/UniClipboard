-- Revert 2026-06-25-000003_add_search_document_render_columns.

ALTER TABLE search_document DROP COLUMN payload_state;
ALTER TABLE search_document DROP COLUMN source_device;
ALTER TABLE search_document DROP COLUMN link_urls;
ALTER TABLE search_document DROP COLUMN file_names;
