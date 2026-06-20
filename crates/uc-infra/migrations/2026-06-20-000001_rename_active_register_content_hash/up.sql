-- Rename the active-clipboard register's content column to `snapshot_hash`.
--
-- The column holds the whole-snapshot identity hash (the value of
-- `SystemClipboardSnapshot::snapshot_hash`, equal to the
-- `clipboard_event.snapshot_hash` column), not a single representation's
-- content hash. Align the column name with that cross-device snapshot
-- identity so the persistence layer and the sync path share one name.
ALTER TABLE active_clipboard_register RENAME COLUMN content_hash TO snapshot_hash;
