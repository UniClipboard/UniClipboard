use crate::db::schema::space_member;
use diesel::prelude::*;

#[derive(Debug, Queryable)]
#[diesel(table_name = space_member)]
pub struct SpaceMemberRow {
    pub device_id: String,
    pub device_name: String,
    pub identity_fingerprint: String,
    pub joined_at: i64,
    pub sync_preferences: String,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = space_member)]
pub struct NewSpaceMemberRow {
    pub device_id: String,
    pub device_name: String,
    pub identity_fingerprint: String,
    pub joined_at: i64,
    pub sync_preferences: String,
}
