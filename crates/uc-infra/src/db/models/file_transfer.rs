use diesel::prelude::*;

use crate::db::schema::file_transfer;

/// Diesel row model for reading from the `file_transfer` table.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = file_transfer)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct FileTransferRow {
    pub transfer_id: String,
    pub entry_id: Option<String>,
    pub file_size: Option<i64>,
    pub attempt_id: Option<String>,
    pub binding_state: String,
    pub receive_item_id: Option<String>,
    pub item_role: Option<String>,
    pub content_hash: Option<String>,
    pub status: String,
    pub source_device: String,
    pub failure_code: Option<String>,
    pub metadata_ciphertext: Vec<u8>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Diesel row model for inserting into the `file_transfer` table.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = file_transfer)]
pub struct NewFileTransferRow {
    pub transfer_id: String,
    pub entry_id: Option<String>,
    pub file_size: Option<i64>,
    pub attempt_id: Option<String>,
    pub binding_state: String,
    pub receive_item_id: Option<String>,
    pub item_role: Option<String>,
    pub content_hash: Option<String>,
    pub status: String,
    pub source_device: String,
    pub failure_code: Option<String>,
    pub metadata_ciphertext: Vec<u8>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
