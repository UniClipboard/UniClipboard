use crate::db::schema::entry_file_set;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = entry_file_set)]
pub struct EntryFileSetRow {
    pub entry_id: String,
    pub line_index: i64,
    pub original_text: String,
    pub kind: String,
    pub content_hash: Option<String>,
    pub blob_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub exclude_reason: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = entry_file_set)]
pub struct NewEntryFileSetRow {
    pub entry_id: String,
    pub line_index: i64,
    pub original_text: String,
    pub kind: String,
    pub content_hash: Option<String>,
    pub blob_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub exclude_reason: Option<String>,
}
