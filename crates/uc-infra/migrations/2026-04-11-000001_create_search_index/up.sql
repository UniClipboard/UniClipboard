-- Create the profile-scoped search index tables for local encrypted search.
--
-- Design notes:
--   - Hard-delete semantic enforced: no soft-delete timestamp column anywhere.
--   - All three tables carry profile_id so search data is fully isolated per profile.
--   - term_tag is a 32-byte BLOB (HMAC-SHA256 output). A CHECK constraint guards length.
--   - index_version allows safe full rebuild when normalization rules change (SIDX-07).
--   - search_blocked gates query execution during rebuild (explicit truthful state).

-- 1. search_document: one row per indexable clipboard entry.
CREATE TABLE IF NOT EXISTS search_document (
    profile_id          TEXT    NOT NULL,
    entry_id            TEXT    NOT NULL,
    event_id            TEXT    NOT NULL,
    active_time_ms      BIGINT  NOT NULL,
    captured_at_ms      BIGINT  NOT NULL,
    file_type           TEXT    NOT NULL,
    file_extensions     TEXT    NOT NULL,
    mime_type           TEXT    NOT NULL,
    indexed_at_ms       BIGINT  NOT NULL,
    index_version       TEXT    NOT NULL,
    text_preview        TEXT,
    PRIMARY KEY (profile_id, entry_id)
);

-- 2. search_posting: one row per (term_tag, entry_id) pair in the inverted index.
--    term_tag is HMAC-SHA256(search_key, normalized_token) — 32 bytes, never plaintext.
CREATE TABLE IF NOT EXISTS search_posting (
    profile_id  TEXT    NOT NULL,
    term_tag    BLOB    NOT NULL,
    entry_id    TEXT    NOT NULL,
    field_mask  INTEGER NOT NULL,
    term_freq   INTEGER NOT NULL CHECK (term_freq > 0),
    PRIMARY KEY (profile_id, term_tag, entry_id),
    CHECK (length(term_tag) = 32)
);

-- 3. search_index_meta: one row per profile, tracks rebuild and version state.
CREATE TABLE IF NOT EXISTS search_index_meta (
    profile_id                  TEXT    PRIMARY KEY,
    index_version               TEXT    NOT NULL,
    search_blocked              BOOLEAN NOT NULL DEFAULT 0,
    last_rebuild_started_at_ms  BIGINT,
    last_rebuild_completed_at_ms BIGINT
);

-- Indexes for common query patterns.
CREATE INDEX IF NOT EXISTS idx_search_document_profile_active_time
    ON search_document (profile_id, active_time_ms DESC);

CREATE INDEX IF NOT EXISTS idx_search_document_profile_file_type
    ON search_document (profile_id, file_type);

CREATE INDEX IF NOT EXISTS idx_search_posting_profile_term
    ON search_posting (profile_id, term_tag);

CREATE INDEX IF NOT EXISTS idx_search_posting_profile_entry
    ON search_posting (profile_id, entry_id);
