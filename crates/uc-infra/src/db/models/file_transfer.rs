use diesel::prelude::*;

use crate::db::schema::file_transfer;

/// Diesel row model for reading from the `file_transfer` table.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = file_transfer)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct FileTransferRow {
    pub transfer_id: String,
    pub entry_id: String,
    pub filename: String,
    pub file_size: Option<i64>,
    pub content_hash: Option<String>,
    pub status: String,
    pub source_device: String,
    pub cached_path: Option<String>,
    pub failure_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Diesel row model for inserting into the `file_transfer` table.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = file_transfer)]
pub struct NewFileTransferRow {
    pub transfer_id: String,
    pub entry_id: String,
    pub filename: String,
    pub file_size: Option<i64>,
    pub content_hash: Option<String>,
    pub status: String,
    pub source_device: String,
    pub cached_path: Option<String>,
    pub failure_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
