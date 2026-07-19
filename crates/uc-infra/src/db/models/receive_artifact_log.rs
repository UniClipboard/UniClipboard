use diesel::prelude::*;

use crate::db::schema::receive_artifact_log;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = receive_artifact_log)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ReceiveArtifactLogRow {
    pub entry_id: String,
    pub attempt_id: String,
    pub phase: String,
    pub resolution: String,
    pub artifact_ciphertext: Vec<u8>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = receive_artifact_log)]
pub struct NewReceiveArtifactLogRow {
    pub entry_id: String,
    pub attempt_id: String,
    pub phase: String,
    pub resolution: String,
    pub artifact_ciphertext: Vec<u8>,
    pub updated_at_ms: i64,
}
