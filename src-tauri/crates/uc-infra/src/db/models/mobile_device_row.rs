use crate::db::schema::mobile_device;
use diesel::prelude::*;

/// `mobile_device` 行的 Diesel 投影。字段顺序与 SELECT * 顺序一致;`token_hash`
/// 在 SQLite 上落 BLOB,Diesel 端对应 `Vec<u8>` —— mapper 负责把它在边界
/// 转回 [u8;32] 的 `TokenHash`。
#[derive(Debug, Queryable)]
#[diesel(table_name = mobile_device)]
pub struct MobileDeviceRow {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    pub token_hash: Vec<u8>,
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
    pub token_hash: Vec<u8>,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}
