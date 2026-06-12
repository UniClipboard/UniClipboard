use crate::db::schema::clipboard_selection;
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = clipboard_selection)]
pub struct ClipboardSelectionRow {
    pub entry_id: String,
    pub primary_rep_id: String,
    pub secondary_rep_ids: String,
    pub preview_rep_id: String,
    pub paste_rep_id: String,
    pub policy_version: String,
}

#[derive(Insertable)]
#[diesel(table_name = clipboard_selection)]
pub struct NewClipboardSelectionRow {
    pub entry_id: String,
    pub primary_rep_id: String,
    pub secondary_rep_ids: String,
    pub preview_rep_id: String,
    pub paste_rep_id: String,
    pub policy_version: String,
}
