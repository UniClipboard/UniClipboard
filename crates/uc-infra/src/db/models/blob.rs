use crate::db::schema::blob;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = blob)]
pub struct BlobRow {
    pub blob_id: String,
    pub storage_path: String,
    pub storage_backend: String,
    pub size_bytes: i64,
    pub content_hash: String,
    pub encryption_algo: Option<String>,
    pub created_at_ms: i64,
    pub compressed_size: Option<i64>,
}

#[derive(Insertable)]
#[diesel(table_name = blob)]
pub struct NewBlobRow {
    pub blob_id: String,
    pub storage_backend: String,
    pub storage_path: String,
    pub encryption_algo: Option<String>,
    pub size_bytes: i64,
    pub content_hash: String,
    pub created_at_ms: i64,
    pub compressed_size: Option<i64>,
}
