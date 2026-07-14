-- Intentionally irreversible. The repair removes legacy plaintext paths and
-- recreates the manifest with encrypted path columns. Restoring the legacy
-- shape would violate the persist-as-ciphertext invariant.
SELECT 1 FROM "cannot downgrade: repaired entry_file_set schema is encryption-only";
