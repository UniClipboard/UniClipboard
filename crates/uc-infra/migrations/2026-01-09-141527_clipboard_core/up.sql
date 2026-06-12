PRAGMA foreign_keys = ON;

CREATE TABLE clipboard_event (
    event_id        TEXT PRIMARY KEY NOT NULL,
    captured_at_ms  BIGINT NOT NULL,
    source_device   TEXT NOT NULL,
    snapshot_hash   TEXT NOT NULL
);

CREATE INDEX idx_clipboard_event_time
ON clipboard_event (captured_at_ms DESC);

CREATE TABLE blob (
    blob_id         TEXT PRIMARY KEY NOT NULL,
    storage_path    TEXT NOT NULL,
    storage_backend TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL,
    content_hash    TEXT NOT NULL UNIQUE,
    encryption_algo TEXT,
    created_at_ms   BIGINT NOT NULL
);

CREATE TABLE clipboard_snapshot_representation (
    id              TEXT PRIMARY KEY NOT NULL,
    event_id        TEXT NOT NULL,
    format_id       TEXT NOT NULL,
    mime_type       TEXT,
    size_bytes      BIGINT NOT NULL,

    inline_data     BLOB,
    blob_id         TEXT,

    CHECK (
      (inline_data IS NOT NULL AND blob_id IS NULL)
      OR
      (inline_data IS NULL AND blob_id IS NOT NULL)
    ),

    FOREIGN KEY(event_id)
      REFERENCES clipboard_event(event_id)
      ON DELETE CASCADE,

    FOREIGN KEY(blob_id)
      REFERENCES blob(blob_id)
      ON DELETE SET NULL
);

CREATE INDEX idx_snapshot_event
ON clipboard_snapshot_representation (event_id);

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

CREATE INDEX idx_entry_time
ON clipboard_entry (created_at_ms DESC);

CREATE TABLE clipboard_selection (
    entry_id            TEXT PRIMARY KEY NOT NULL,

    primary_rep_id      TEXT NOT NULL,
    secondary_rep_ids   TEXT NOT NULL, -- xxx,yyy,zzz
    preview_rep_id      TEXT NOT NULL,
    paste_rep_id        TEXT NOT NULL,

    policy_version      TEXT NOT NULL,

    FOREIGN KEY(entry_id)
      REFERENCES clipboard_entry(entry_id)
      ON DELETE CASCADE,

    FOREIGN KEY(primary_rep_id)
      REFERENCES clipboard_snapshot_representation(id),

    FOREIGN KEY(preview_rep_id)
      REFERENCES clipboard_snapshot_representation(id),

    FOREIGN KEY(paste_rep_id)
      REFERENCES clipboard_snapshot_representation(id)
);
