-- Add render-metadata columns to the search index document table.
--
-- These mirror clipboard render fields that are stable at capture time, so the
-- search result can render a card without a per-entry lazy fetch:
--   file_names    - display names from a file:// uri-list (JSON array)
--   link_urls     - http/https URLs, same detection as the `link` tag (JSON array)
--   source_device - originating device id, resolved from the clipboard event
--   payload_state - 'Lost' when the paste payload is unrecoverable, else NULL
--
-- Image dimensions and file sizes are intentionally NOT stored here: thumbnail
-- metadata is generated asynchronously after live indexing (so a live row would
-- hold NULL while a rebuilt row holds a value, breaking live/rebuild parity), and
-- file sizes are a volatile filesystem stat. Both stay lazy, fetched by entry id.
--
-- Bumping CURRENT_INDEX_VERSION to search-v5 forces a full rebuild that backfills
-- file_names / link_urls / source_device / payload_state for existing rows.

ALTER TABLE search_document ADD COLUMN file_names TEXT NOT NULL DEFAULT '[]';
ALTER TABLE search_document ADD COLUMN link_urls TEXT NOT NULL DEFAULT '[]';
ALTER TABLE search_document ADD COLUMN source_device TEXT;
ALTER TABLE search_document ADD COLUMN payload_state TEXT;
