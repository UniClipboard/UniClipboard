-- Re-add the column as a nullable plaintext TEXT column.
--
-- The dropped title data is unrecoverable (and was pure denormalized redundancy
-- derived from each entry's first text representation), so the restored column
-- comes back empty. An older binary that expects `title` will read NULL and fall
-- back to the inline representation preview, which is the same behavior a fresh
-- capture produces before its title would have been computed.
ALTER TABLE clipboard_entry ADD COLUMN title TEXT;
