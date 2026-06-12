use crate::db::schema::clipboard_snapshot_representation;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = clipboard_snapshot_representation)]
pub struct SnapshotRepresentationRow {
    pub id: String,
    pub event_id: String,
    pub format_id: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub inline_data: Option<Vec<u8>>,
    pub blob_id: Option<String>,
    pub payload_state: String,
    pub last_error: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = clipboard_snapshot_representation)]
pub struct NewSnapshotRepresentationRow {
    pub id: String,
    pub event_id: String,
    pub format_id: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub inline_data: Option<Vec<u8>>,
    pub blob_id: Option<String>,
    pub payload_state: String,
    pub last_error: Option<String>,
}
