use crate::db::schema::clipboard_entry;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = clipboard_entry)]
pub struct ClipboardEntryRow {
    pub entry_id: String,
    pub event_id: String,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub title: Option<String>,
    pub total_size: i64,
    pub pinned: bool,
    pub deleted_at_ms: Option<i64>,
    pub delivery_tracked: bool,
}

#[derive(Insertable)]
#[diesel(table_name = clipboard_entry)]
pub struct NewClipboardEntryRow {
    pub entry_id: String,
    pub event_id: String,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub title: Option<String>,
    pub total_size: i64,
    pub pinned: bool,
    pub delivery_tracked: bool,
}
