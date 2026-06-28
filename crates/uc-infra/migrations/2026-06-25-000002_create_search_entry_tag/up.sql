-- Create the entry-to-tag membership table for the search index.
--
-- search_entry_tag holds the derived/mirrored tag membership for each indexed
-- entry: builtin rule tags (e.g. `link`) and user-state tags (e.g. `favorited`).
-- It is pure derived data, rebuilt from entry content and user-state alongside
-- search_document / search_posting, so it carries only the identity triple and
-- is safe to drop/recreate. Bumping CURRENT_INDEX_VERSION to search-v4 forces a
-- full rebuild that repopulates this table.

CREATE TABLE IF NOT EXISTS search_entry_tag (
    profile_id  TEXT NOT NULL,
    entry_id    TEXT NOT NULL,
    tag_id      TEXT NOT NULL,
    PRIMARY KEY (profile_id, entry_id, tag_id)
);

-- Reverse lookup: all entries carrying a given tag (tag filter path).
CREATE INDEX IF NOT EXISTS idx_entry_tag_by_tag
    ON search_entry_tag (profile_id, tag_id);
