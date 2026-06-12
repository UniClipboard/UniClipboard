CREATE TABLE file_transfer (
    transfer_id TEXT PRIMARY KEY NOT NULL,
    filename TEXT NOT NULL,
    file_size BIGINT NOT NULL,
    content_hash TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    source_device TEXT NOT NULL,
    batch_id TEXT,
    cached_path TEXT,
    created_at_ms BIGINT NOT NULL,
    updated_at_ms BIGINT NOT NULL
);

CREATE INDEX idx_file_transfer_status ON file_transfer(status);
CREATE INDEX idx_file_transfer_batch ON file_transfer(batch_id);
CREATE INDEX idx_file_transfer_created ON file_transfer(created_at_ms);
