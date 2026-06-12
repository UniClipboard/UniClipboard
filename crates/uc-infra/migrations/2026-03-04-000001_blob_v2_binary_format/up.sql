-- Delete all existing blob records (incompatible old JSON format)
-- FK: clipboard_snapshot_representation.blob_id -> SET NULL handled by app logic
DELETE FROM blob;

-- Add compressed_size column for storage metrics
-- NULL for inline data or legacy entries
ALTER TABLE blob ADD COLUMN compressed_size BIGINT;
