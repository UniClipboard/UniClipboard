DROP INDEX IF EXISTS idx_entry_active_time;

CREATE TABLE clipboard_entry_backup AS
SELECT
    entry_id,
    event_id,
    created_at_ms,
    title,
    total_size,
    pinned,
    deleted_at_ms
FROM clipboard_entry;

DROP TABLE clipboard_entry;

CREATE TABLE clipboard_entry (
    entry_id        TEXT PRIMARY KEY NOT NULL,
    event_id        TEXT NOT NULL UNIQUE,

    created_at_ms   BIGINT NOT NULL,
    title           TEXT,
    total_size      BIGINT NOT NULL,

    pinned          BOOLEAN NOT NULL DEFAULT 0,
    deleted_at_ms   BIGINT,

    FOREIGN KEY(event_id)
      REFERENCES clipboard_event(event_id)
);

INSERT INTO clipboard_entry (
    entry_id,
    event_id,
    created_at_ms,
    title,
    total_size,
    pinned,
    deleted_at_ms
)
SELECT
    entry_id,
    event_id,
    created_at_ms,
    title,
    total_size,
    pinned,
    deleted_at_ms
FROM clipboard_entry_backup;

DROP TABLE clipboard_entry_backup;

CREATE INDEX idx_entry_time
ON clipboard_entry (created_at_ms DESC);
