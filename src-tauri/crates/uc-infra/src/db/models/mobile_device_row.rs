use crate::db::schema::mobile_device;
use diesel::prelude::*;

/// `mobile_device` 行的 Diesel 投影(v3 SyncClipboard 兼容版)。字段顺序与
/// `SELECT *` 一致;`username` / `password_hash` 都是 TEXT,与 schema.rs
/// 严格匹配,mapper 负责把它们封装到 domain 实体。
#[derive(Debug, Queryable)]
#[diesel(table_name = mobile_device)]
pub struct MobileDeviceRow {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    pub username: String,
    pub password_hash: String,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = mobile_device)]
pub struct NewMobileDeviceRow {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    pub username: String,
    pub password_hash: String,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}
