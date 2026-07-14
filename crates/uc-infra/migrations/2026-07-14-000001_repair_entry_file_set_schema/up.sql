-- Repair entry_file_set databases that recorded migration version
-- 20260710000001 for a different branch migration. Diesel identifies applied
-- migrations only by version, so the path-encryption migration could be
-- skipped while later migrations still ran, leaving a mixed legacy schema.
--
-- The manifest is a rebuildable cache and its path fields cannot be migrated
-- without the unlocked MasterKey. Recreate the table in its canonical encrypted
-- shape; subsequent clipboard captures repopulate it.
DROP TABLE IF EXISTS entry_file_set;

CREATE TABLE entry_file_set (
    entry_id         TEXT   NOT NULL,
    line_index       BIGINT NOT NULL,
    kind             TEXT   NOT NULL,
    content_hash     TEXT,
    blob_id          TEXT,
    size_bytes       BIGINT,
    exclude_reason   TEXT,
    original_text_ct BLOB,
    root_index       BIGINT,
    relative_path_ct BLOB,
    kind_tag         TEXT,
    root_name_ct     BLOB,
    PRIMARY KEY (entry_id, line_index),
    FOREIGN KEY (entry_id) REFERENCES clipboard_entry(entry_id) ON DELETE CASCADE
);
