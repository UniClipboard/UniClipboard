-- Revert to the narrow (profile_id, tag_id) tag-filter index.

DROP INDEX IF EXISTS idx_entry_tag_by_tag;

CREATE INDEX IF NOT EXISTS idx_entry_tag_by_tag
    ON search_entry_tag (profile_id, tag_id);
