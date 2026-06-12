CREATE TABLE file_transfer_events (
    id             INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    transfer_id    TEXT    NOT NULL,
    sequence       INTEGER NOT NULL,
    event_type     TEXT    NOT NULL,
    payload_json   TEXT    NOT NULL,
    occurred_at_ms BIGINT  NOT NULL,
    UNIQUE (transfer_id, sequence)
);

CREATE INDEX idx_file_transfer_events_transfer_sequence
    ON file_transfer_events(transfer_id, sequence);
