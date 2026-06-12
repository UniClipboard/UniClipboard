-- WARNING: This is a destructive rollback.
-- Rolling back loses entry_id and failure_reason data permanently.
-- This is a schema reshape (not an incremental column add) and
-- data loss is expected on rollback.

CREATE TABLE file_transfer_old (
    transfer_id   TEXT    PRIMARY KEY NOT NULL,
    filename      TEXT    NOT NULL,
    file_size     BIGINT  NOT NULL,
    content_hash  TEXT    NOT NULL,
    status        TEXT    NOT NULL DEFAULT 'pending',
    source_device TEXT    NOT NULL,
    batch_id      TEXT,
    cached_path   TEXT,
    created_at_ms BIGINT  NOT NULL,
    updated_at_ms BIGINT  NOT NULL
);

INSERT INTO file_transfer_old (
    transfer_id, filename, file_size, content_hash,
    status, source_device, batch_id, cached_path,
    created_at_ms, updated_at_ms
)
SELECT
    transfer_id, filename,
    COALESCE(file_size, 0),
    COALESCE(content_hash, ''),
    status, source_device,
    entry_id,
    cached_path,
    created_at_ms, updated_at_ms
FROM file_transfer;

DROP TABLE file_transfer;
ALTER TABLE file_transfer_old RENAME TO file_transfer;

CREATE INDEX idx_file_transfer_status ON file_transfer(status);
CREATE INDEX idx_file_transfer_batch ON file_transfer(batch_id);
CREATE INDEX idx_file_transfer_created ON file_transfer(created_at_ms);
