-- Revert blob V2 migration (cannot recover deleted blobs)
-- SQLite >= 3.35 supports DROP COLUMN
ALTER TABLE blob DROP COLUMN compressed_size;
