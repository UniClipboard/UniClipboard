DELETE FROM file_transfer WHERE entry_id IS NULL;

ALTER TABLE file_transfer RENAME TO file_transfer_with_provisional;

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

INSERT INTO file_transfer (
    transfer_id,
    entry_id,
    file_size,
    attempt_id,
    content_hash,
    status,
    source_device,
    failure_code,
    metadata_ciphertext,
    created_at_ms,
    updated_at_ms
)
SELECT
    transfer_id,
    entry_id,
    file_size,
    attempt_id,
    content_hash,
    status,
    source_device,
    failure_code,
    metadata_ciphertext,
    created_at_ms,
    updated_at_ms
FROM file_transfer_with_provisional;

DROP TABLE file_transfer_with_provisional;

CREATE INDEX idx_file_transfer_status_v2 ON file_transfer(status);
CREATE INDEX idx_file_transfer_entry_v2 ON file_transfer(entry_id);
CREATE INDEX idx_file_transfer_created_v2 ON file_transfer(created_at_ms);
CREATE INDEX idx_file_transfer_entry_attempt_v2 ON file_transfer(entry_id, attempt_id);
