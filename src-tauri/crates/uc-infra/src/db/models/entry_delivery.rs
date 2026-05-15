use crate::db::schema::clipboard_entry_delivery;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = clipboard_entry_delivery)]
pub struct EntryDeliveryRow {
    pub entry_id: String,
    pub target_device_id: String,
    pub status: String,
    pub reason_detail: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = clipboard_entry_delivery)]
pub struct NewEntryDeliveryRow {
    pub entry_id: String,
    pub target_device_id: String,
    pub status: String,
    pub reason_detail: Option<String>,
    pub updated_at_ms: i64,
}
