use crate::db::schema::trusted_peer;
use diesel::prelude::*;

#[derive(Debug, Queryable)]
#[diesel(table_name = trusted_peer)]
pub struct TrustedPeerRow {
    pub peer_device_id: String,
    pub local_device_id: String,
    pub peer_fingerprint: String,
    pub trusted_at: i64,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = trusted_peer)]
pub struct NewTrustedPeerRow {
    pub peer_device_id: String,
    pub local_device_id: String,
    pub peer_fingerprint: String,
    pub trusted_at: i64,
}
