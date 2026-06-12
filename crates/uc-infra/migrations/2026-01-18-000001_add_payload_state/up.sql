-- Rebuild clipboard_snapshot_representation to relax constraints and add payload_state.

CREATE TABLE clipboard_selection_backup AS
SELECT
    entry_id,
    primary_rep_id,
    secondary_rep_ids,
    preview_rep_id,
    paste_rep_id,
    policy_version
FROM clipboard_selection;

DROP TABLE clipboard_selection;

CREATE TABLE clipboard_snapshot_representation_rebuild (
    id              TEXT PRIMARY KEY NOT NULL,
    event_id        TEXT NOT NULL,
    format_id       TEXT NOT NULL,
    mime_type       TEXT,
    size_bytes      BIGINT NOT NULL,

    inline_data     BLOB,
    blob_id         TEXT,

    payload_state   TEXT NOT NULL DEFAULT 'Staged',
    last_error      TEXT,

    CHECK (inline_data IS NULL OR blob_id IS NULL),
    CHECK (payload_state IN (
        'Inline',
        'BlobReady',
        'Staged',
        'Processing',
        'Failed',
        'Lost'
    )),

    FOREIGN KEY(event_id)
      REFERENCES clipboard_event(event_id)
      ON DELETE CASCADE,

    FOREIGN KEY(blob_id)
      REFERENCES blob(blob_id)
      ON DELETE SET NULL
);

INSERT INTO clipboard_snapshot_representation_rebuild (
    id,
    event_id,
    format_id,
    mime_type,
    size_bytes,
    inline_data,
    blob_id,
    payload_state,
    last_error
)
SELECT
    id,
    event_id,
    format_id,
    mime_type,
    size_bytes,
    inline_data,
    blob_id,
    CASE
        WHEN inline_data IS NOT NULL THEN 'Inline'
        WHEN blob_id IS NOT NULL THEN 'BlobReady'
        ELSE 'Staged'
    END,
    NULL
FROM clipboard_snapshot_representation;

DROP TABLE clipboard_snapshot_representation;

ALTER TABLE clipboard_snapshot_representation_rebuild
RENAME TO clipboard_snapshot_representation;

CREATE INDEX idx_snapshot_event
ON clipboard_snapshot_representation (event_id);

CREATE TABLE clipboard_selection (
    entry_id            TEXT PRIMARY KEY NOT NULL,

    primary_rep_id      TEXT NOT NULL,
    secondary_rep_ids   TEXT NOT NULL,
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

INSERT INTO clipboard_selection (
    entry_id,
    primary_rep_id,
    secondary_rep_ids,
    preview_rep_id,
    paste_rep_id,
    policy_version
)
SELECT
    entry_id,
    primary_rep_id,
    secondary_rep_ids,
    preview_rep_id,
    paste_rep_id,
    policy_version
FROM clipboard_selection_backup;

DROP TABLE clipboard_selection_backup;
