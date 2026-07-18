CREATE TABLE directory_publish_log (
    entry_id            TEXT    NOT NULL,
    attempt_id          TEXT    NOT NULL,
    phase               TEXT    NOT NULL,
    root_map_ciphertext BLOB,
    partial_publication INTEGER NOT NULL DEFAULT 0,
    partial_root_count  INTEGER NOT NULL DEFAULT 0,
    landed              INTEGER NOT NULL DEFAULT 0,
    updated_at_ms       BIGINT  NOT NULL,
    PRIMARY KEY (entry_id, attempt_id)
);
