-- Upgrade file_transfer for metadata-first durable tracking.
--
-- Changes:
--   - Add entry_id (NOT NULL) linking each transfer to its owning clipboard entry
--   - Add failure_reason (nullable) for failed state diagnostics
--   - Make file_size nullable (unknown until announce metadata arrives)
--   - Make content_hash nullable (unknown until announce metadata arrives)
--   - Remove batch_id (replaced by entry_id)
--   - Add indexes for entry_id and (entry_id, status)
--
-- SQLite lacks ALTER COLUMN / DROP COLUMN before 3.35, so we use the
-- standard create-copy-drop-rename approach to preserve existing rows.

-- 1. Create the new table shape
CREATE TABLE file_transfer_new (
    transfer_id   TEXT    PRIMARY KEY NOT NULL,
    entry_id      TEXT    NOT NULL,
    filename      TEXT    NOT NULL,
    file_size     BIGINT,
    content_hash  TEXT,
    status        TEXT    NOT NULL DEFAULT 'pending',
    source_device TEXT    NOT NULL,
    cached_path   TEXT,
    failure_reason TEXT,
    created_at_ms BIGINT  NOT NULL,
    updated_at_ms BIGINT  NOT NULL
);

-- 2. Copy existing rows. entry_id defaults to batch_id when available,
--    otherwise falls back to transfer_id so the NOT NULL constraint holds.
INSERT INTO file_transfer_new (
    transfer_id, entry_id, filename, file_size, content_hash,
    status, source_device, cached_path, failure_reason,
    created_at_ms, updated_at_ms
)
SELECT
    transfer_id,
    COALESCE(batch_id, transfer_id),
    filename,
    file_size,
    content_hash,
    status,
    source_device,
    cached_path,
    NULL,
    created_at_ms,
    updated_at_ms
FROM file_transfer;

-- 3. Drop old table and indexes
DROP TABLE file_transfer;

-- 4. Rename new table into place
ALTER TABLE file_transfer_new RENAME TO file_transfer;

-- 5. Recreate status index and add new indexes
CREATE INDEX idx_file_transfer_status ON file_transfer(status);
CREATE INDEX idx_file_transfer_entry_id ON file_transfer(entry_id);
CREATE INDEX idx_file_transfer_entry_status ON file_transfer(entry_id, status);
CREATE INDEX idx_file_transfer_created ON file_transfer(created_at_ms);
