DROP TRIGGER IF EXISTS file_transfer_privacy_purge_completed;
DROP TABLE IF EXISTS file_transfer_privacy_maintenance;

DROP TABLE IF EXISTS file_transfer_events;
CREATE TABLE file_transfer_events (
    id             INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    transfer_id    TEXT NOT NULL,
    sequence       INTEGER NOT NULL,
    event_type     TEXT NOT NULL,
    payload_json   TEXT NOT NULL,
    occurred_at_ms BIGINT NOT NULL,
    UNIQUE (transfer_id, sequence)
);
CREATE INDEX idx_file_transfer_events_transfer_sequence
    ON file_transfer_events(transfer_id, sequence);

DROP TABLE IF EXISTS file_transfer;
CREATE TABLE file_transfer (
    transfer_id    TEXT PRIMARY KEY NOT NULL,
    entry_id       TEXT NOT NULL,
    filename       TEXT NOT NULL,
    file_size      BIGINT,
    attempt_id     TEXT,
    content_hash   TEXT,
    status         TEXT NOT NULL DEFAULT 'pending',
    source_device  TEXT NOT NULL,
    cached_path    TEXT,
    failure_reason TEXT,
    created_at_ms  BIGINT NOT NULL,
    updated_at_ms  BIGINT NOT NULL
);
CREATE INDEX idx_file_transfer_status ON file_transfer(status);
CREATE INDEX idx_file_transfer_entry ON file_transfer(entry_id);
CREATE INDEX idx_file_transfer_created ON file_transfer(created_at_ms);
CREATE INDEX idx_file_transfer_entry_attempt ON file_transfer(entry_id, attempt_id);
