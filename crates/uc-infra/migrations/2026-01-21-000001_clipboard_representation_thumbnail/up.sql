CREATE TABLE clipboard_representation_thumbnail (
    representation_id TEXT PRIMARY KEY NOT NULL,
    thumbnail_blob_id TEXT NOT NULL,
    thumbnail_mime_type TEXT NOT NULL,
    original_width INTEGER NOT NULL,
    original_height INTEGER NOT NULL,
    original_size_bytes BIGINT NOT NULL,
    created_at_ms BIGINT
);
