use diesel::prelude::*;

use crate::db::schema::clipboard_representation_thumbnail;

#[derive(Debug, Clone, Queryable)]
#[diesel(table_name = clipboard_representation_thumbnail)]
pub struct ClipboardRepresentationThumbnailRow {
    pub representation_id: String,
    pub thumbnail_blob_id: String,
    pub thumbnail_mime_type: String,
    pub original_width: i32,
    pub original_height: i32,
    pub original_size_bytes: i64,
    pub created_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = clipboard_representation_thumbnail)]
pub struct NewClipboardRepresentationThumbnailRow {
    pub representation_id: String,
    pub thumbnail_blob_id: String,
    pub thumbnail_mime_type: String,
    pub original_width: i32,
    pub original_height: i32,
    pub original_size_bytes: i64,
    pub created_at_ms: Option<i64>,
}
