use crate::db::schema::peer_address;
use diesel::prelude::*;

#[derive(Debug, Queryable)]
#[diesel(table_name = peer_address)]
pub struct PeerAddressRow {
    pub device_id: String,
    pub addr_blob: Vec<u8>,
    pub observed_at: i64,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = peer_address)]
pub struct NewPeerAddressRow {
    pub device_id: String,
    pub addr_blob: Vec<u8>,
    pub observed_at: i64,
}
