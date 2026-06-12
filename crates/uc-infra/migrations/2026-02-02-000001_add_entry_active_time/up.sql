ALTER TABLE clipboard_entry
ADD COLUMN active_time_ms BIGINT NOT NULL DEFAULT 0;

UPDATE clipboard_entry
SET active_time_ms = created_at_ms
WHERE active_time_ms = 0;

CREATE INDEX idx_entry_active_time
ON clipboard_entry (active_time_ms DESC);
