-- Add a full character-count render column to the search index document table.
--
-- `text_preview` is capped at 200 chars, so the UI could only ever derive a
-- "200 characters" label from it. `char_count` carries the real total length of
-- the entry's primary text content, captured at index time from the full text
-- (the same source `text_preview` is truncated from), so a history card can show
-- the true total without a per-entry lazy fetch.
--
-- Nullable: an entry with no inline text (image / file / payload not inline) has
-- no measurable text length, so the column stays NULL and the UI falls back to
-- the preview length.
--
-- The active CURRENT_INDEX_VERSION (search-v8) rebuild backfills char_count for
-- existing rows alongside the other render columns.

ALTER TABLE search_document ADD COLUMN char_count INTEGER;
