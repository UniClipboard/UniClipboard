ALTER TABLE file_transfer RENAME TO file_transfer_plaintext_legacy;

CREATE TABLE file_transfer (
    transfer_id         TEXT PRIMARY KEY NOT NULL,
    entry_id            TEXT NOT NULL,
    file_size           BIGINT,
    attempt_id          TEXT,
    content_hash        TEXT,
    status              TEXT NOT NULL DEFAULT 'pending',
    source_device       TEXT NOT NULL,
    failure_code        TEXT,
    metadata_ciphertext BLOB NOT NULL,
    created_at_ms       BIGINT NOT NULL,
    updated_at_ms       BIGINT NOT NULL
);

CREATE INDEX idx_file_transfer_status_v2 ON file_transfer(status);
CREATE INDEX idx_file_transfer_entry_v2 ON file_transfer(entry_id);
CREATE INDEX idx_file_transfer_created_v2 ON file_transfer(created_at_ms);
CREATE INDEX idx_file_transfer_entry_attempt_v2 ON file_transfer(entry_id, attempt_id);

ALTER TABLE file_transfer_events RENAME TO file_transfer_events_plaintext_legacy;

CREATE TABLE file_transfer_events (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    transfer_id        TEXT NOT NULL,
    sequence           INTEGER NOT NULL,
    event_type         TEXT NOT NULL,
    payload_ciphertext BLOB NOT NULL,
    occurred_at_ms     BIGINT NOT NULL,
    UNIQUE (transfer_id, sequence)
);

CREATE INDEX idx_file_transfer_events_transfer_sequence_v2
    ON file_transfer_events(transfer_id, sequence);

CREATE TABLE file_transfer_privacy_maintenance (
    id            INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    state         TEXT NOT NULL CHECK (state IN ('pending_physical_purge', 'completed')),
    updated_at_ms BIGINT NOT NULL
);

INSERT INTO file_transfer_privacy_maintenance (id, state, updated_at_ms)
VALUES (1, 'pending_physical_purge', 0);

DROP TABLE file_transfer_plaintext_legacy;
DROP TABLE file_transfer_events_plaintext_legacy;
