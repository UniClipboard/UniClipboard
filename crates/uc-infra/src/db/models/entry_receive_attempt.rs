use diesel::prelude::*;

use crate::db::schema::entry_receive_attempt;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = entry_receive_attempt)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct EntryReceiveAttemptRow {
    pub entry_id: String,
    pub current_attempt_id: String,
    pub attempt_state: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = entry_receive_attempt)]
pub struct NewEntryReceiveAttemptRow {
    pub entry_id: String,
    pub current_attempt_id: String,
    pub attempt_state: String,
    pub updated_at_ms: i64,
}
