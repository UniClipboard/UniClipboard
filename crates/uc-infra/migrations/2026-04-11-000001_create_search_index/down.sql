-- Rollback the profile-scoped search index tables.
-- WARNING: This is a destructive rollback — all indexed search data will be lost.

-- Drop indexes first (required before dropping tables in some SQLite versions).
DROP INDEX IF EXISTS idx_search_posting_profile_entry;
DROP INDEX IF EXISTS idx_search_posting_profile_term;
DROP INDEX IF EXISTS idx_search_document_profile_file_type;
DROP INDEX IF EXISTS idx_search_document_profile_active_time;

-- Drop tables in reverse dependency order.
DROP TABLE IF EXISTS search_index_meta;
DROP TABLE IF EXISTS search_posting;
DROP TABLE IF EXISTS search_document;
