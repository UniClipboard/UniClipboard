-- Intentionally irreversible (see up.sql).
--
-- Encrypted render payloads cannot be decrypted inside a migration context, so a
-- rollback would recreate empty plaintext columns and silently drop the render
-- data. Failing loudly is safer than corrupting the search index. To go back to
-- an older binary, delete the search index and let the older version rebuild it.
--
-- The quoted identifier is not a real table; SQLite reports it as the error
-- message so the intent is visible in logs.
SELECT 1 FROM "cannot downgrade: search render-column encryption is irreversible; delete the search index to rebuild on the older version";
