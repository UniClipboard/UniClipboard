use crate::db::schema::clipboard_event;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = clipboard_event)]
pub struct ClipboardEventRow {
    pub event_id: String,
    pub captured_at_ms: i64,
    pub source_device: String,
    pub snapshot_hash: String,
}

#[derive(Insertable)]
#[diesel(table_name = clipboard_event)]
pub struct NewClipboardEventRow {
    pub event_id: String,
    pub captured_at_ms: i64,
    pub source_device: String,
    pub snapshot_hash: String,
}
