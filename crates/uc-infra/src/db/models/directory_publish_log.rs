use diesel::prelude::*;

use crate::db::schema::directory_publish_log;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = directory_publish_log)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct DirectoryPublishLogRow {
    pub entry_id: String,
    pub attempt_id: String,
    pub phase: String,
    pub root_map_ciphertext: Option<Vec<u8>>,
    pub partial_publication: bool,
    pub partial_root_count: i32,
    pub landed: bool,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = directory_publish_log)]
pub struct NewDirectoryPublishLogRow {
    pub entry_id: String,
    pub attempt_id: String,
    pub phase: String,
    pub root_map_ciphertext: Option<Vec<u8>>,
    pub partial_publication: bool,
    pub partial_root_count: i32,
    pub landed: bool,
    pub updated_at_ms: i64,
}
