-- Add a file-paths render column to the search index document table.
--
-- `file_names` already mirrors the display names from a `file://` uri-list, but
-- the local filesystem paths were dropped at index time. `file_paths` carries
-- those paths (aligned with `file_names` by index, JSON array), captured at
-- index time from the same uri-list, so a search/browse row can resolve a
-- file's on-disk location without a per-entry lazy fetch.
--
-- Empty (`'[]'`) when the entry references no resolvable local files (text /
-- image / payload not inline). Capture-time stable, mirrored as a render column
-- alongside the other `search_document` render columns.
--
-- Bumping CURRENT_INDEX_VERSION to search-v9 forces a full rebuild that
-- backfills file_paths for existing rows.

ALTER TABLE search_document ADD COLUMN file_paths TEXT NOT NULL DEFAULT '[]';
