-- Make the tag-filter index covering for the push-down tag subquery.
--
-- The filter-only push-down resolves tag membership with
--   SELECT entry_id FROM search_entry_tag WHERE profile_id = ? AND tag_id IN (...)
-- The previous index (profile_id, tag_id) is selective on tag_id but does not
-- carry entry_id, so SQLite preferred the primary-key autoindex
-- (profile_id, entry_id, tag_id) — covering but NOT selective on tag_id (it
-- scans every tag row for the profile). Widening the index to
-- (profile_id, tag_id, entry_id) makes it both selective and covering, so the
-- planner uses it and the subquery seeks only the matching tag rows.
--
-- Pure query-plan optimization over identical derived data: no index_version
-- bump needed (the rebuild does not depend on this index).

DROP INDEX IF EXISTS idx_entry_tag_by_tag;

CREATE INDEX IF NOT EXISTS idx_entry_tag_by_tag
    ON search_entry_tag (profile_id, tag_id, entry_id);
