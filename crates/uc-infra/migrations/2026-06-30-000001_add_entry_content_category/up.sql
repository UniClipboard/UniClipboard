-- Persist the single physical content category chosen at capture time.
--
-- This is the authority used by browse and search projections. Historical rows
-- are backfilled best-effort from representation MIME presence with the same
-- high-level precedence: file > image > rich_text > text > other.

ALTER TABLE clipboard_entry
ADD COLUMN content_category TEXT NOT NULL DEFAULT 'other';

UPDATE clipboard_entry
SET content_category = COALESCE(
    (
        SELECT CASE MAX(
            CASE
                WHEN (
                    lower(COALESCE(r.mime_type, '')) LIKE 'text/uri-list%'
                    OR lower(COALESCE(r.mime_type, '')) LIKE 'file/uri-list%'
                    OR lower(COALESCE(r.format_id, '')) = 'files'
                ) AND instr(lower(CAST(COALESCE(r.inline_data, X'') AS TEXT)), 'file://') > 0 THEN 40
                WHEN lower(COALESCE(r.mime_type, '')) LIKE 'image/%' THEN 30
                WHEN lower(COALESCE(r.mime_type, '')) LIKE 'text/html%' THEN 20
                WHEN lower(COALESCE(r.mime_type, '')) = 'text/rtf' THEN 20
                WHEN lower(COALESCE(r.mime_type, '')) LIKE 'text/rtf;%' THEN 20
                WHEN lower(COALESCE(r.mime_type, '')) LIKE 'text/plain%' THEN 10
                WHEN lower(COALESCE(r.mime_type, '')) LIKE 'text/%' THEN 10
                ELSE 0
            END
        )
            WHEN 40 THEN 'file'
            WHEN 30 THEN 'image'
            WHEN 20 THEN 'rich_text'
            WHEN 10 THEN 'text'
            ELSE 'other'
        END
        FROM clipboard_snapshot_representation r
        WHERE r.event_id = clipboard_entry.event_id
    ),
    'other'
);
