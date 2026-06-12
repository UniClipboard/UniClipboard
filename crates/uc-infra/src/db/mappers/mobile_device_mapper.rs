//! `MobileDeviceRowMapper` —— `MobileDevice` ↔ sqlite 行的边界转换
//! (v3 SyncClipboard 兼容版)。
//!
//! v1/v2 这里曾把 `token_hash`(BLOB Vec<u8> ↔ `[u8; 32]`)做严格长度校验,
//! v3 切到 username + password_hash(都是 TEXT)后,字段就没什么需要"长度
//! 兜底"的了 —— mapper 只剩下纯字符串拷贝 + `client_type` 的枚举翻译。
//! 即便 password_hash 字符串损坏,鉴权时调 `PasswordHasherPort::verify`
//! 才会在那里报错,mapper 不替它做格式校验。
//!
//! `client_type` 走 `MobileClientType::as_wire_str` / `from_wire_str` 这对
//! 函数,陌生值按行损坏处理(adapter 层 fail-loud,而不是悄悄落 default)。

use anyhow::{anyhow, Result};

use uc_core::mobile_sync::{MobileClientType, MobileDevice, MobileDeviceId};

use crate::db::models::{MobileDeviceRow, NewMobileDeviceRow};
use crate::db::ports::{InsertMapper, RowMapper};

pub struct MobileDeviceRowMapper;

impl InsertMapper<MobileDevice, NewMobileDeviceRow> for MobileDeviceRowMapper {
    fn to_row(&self, domain: &MobileDevice) -> Result<NewMobileDeviceRow> {
        Ok(NewMobileDeviceRow {
            device_id: domain.device_id.as_str().to_string(),
            label: domain.label.clone(),
            client_type: domain.client_type.as_wire_str().to_string(),
            username: domain.username.clone(),
            password_hash: domain.password_hash.clone(),
            created_at_ms: domain.created_at_ms,
            last_seen_at_ms: domain.last_seen_at_ms,
            last_seen_ip: domain.last_seen_ip.clone(),
            reported_name: domain.reported_name.clone(),
            reported_os: domain.reported_os.clone(),
        })
    }
}

impl RowMapper<MobileDeviceRow, MobileDevice> for MobileDeviceRowMapper {
    fn to_domain(&self, row: &MobileDeviceRow) -> Result<MobileDevice> {
        let client_type = MobileClientType::from_wire_str(&row.client_type).ok_or_else(|| {
            anyhow!(
                "unknown client_type {:?} in mobile_device row {}",
                row.client_type,
                row.device_id
            )
        })?;

        Ok(MobileDevice {
            device_id: MobileDeviceId::new(row.device_id.clone()),
            label: row.label.clone(),
            client_type,
            username: row.username.clone(),
            password_hash: row.password_hash.clone(),
            created_at_ms: row.created_at_ms,
            last_seen_at_ms: row.last_seen_at_ms,
            last_seen_ip: row.last_seen_ip.clone(),
            reported_name: row.reported_name.clone(),
            reported_os: row.reported_os.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(suffix: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new("did_abc"),
            label: "iPhone".to_string(),
            client_type: MobileClientType::IosShortcut,
            username: format!("mobile_{suffix}"),
            password_hash: format!("$argon2id$v=19$m=65536,t=3,p=4$salt$hash{suffix}"),
            created_at_ms: 1_700_000_000_000,
            last_seen_at_ms: Some(1_700_000_001_000),
            last_seen_ip: Some("192.168.1.5".into()),
            reported_name: Some("iPhone 15".into()),
            reported_os: Some("iOS 18".into()),
        }
    }

    #[test]
    fn round_trip_through_row_preserves_all_fields() {
        let mapper = MobileDeviceRowMapper;
        let original = fixture("0001");

        let new_row = mapper.to_row(&original).expect("to_row");
        let row = MobileDeviceRow {
            device_id: new_row.device_id,
            label: new_row.label,
            client_type: new_row.client_type,
            username: new_row.username,
            password_hash: new_row.password_hash,
            created_at_ms: new_row.created_at_ms,
            last_seen_at_ms: new_row.last_seen_at_ms,
            last_seen_ip: new_row.last_seen_ip,
            reported_name: new_row.reported_name,
            reported_os: new_row.reported_os,
        };

        let restored = mapper.to_domain(&row).expect("to_domain");
        assert_eq!(restored, original);
    }

    #[test]
    fn round_trip_with_all_optional_none_fields() {
        let mapper = MobileDeviceRowMapper;
        let original = MobileDevice {
            device_id: MobileDeviceId::new("did_min"),
            label: "min".into(),
            client_type: MobileClientType::IosShortcut,
            username: "mobile_min".into(),
            password_hash: "$argon2id$v=19$m=64,t=1,p=1$AAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let new_row = mapper.to_row(&original).unwrap();
        let row = MobileDeviceRow {
            device_id: new_row.device_id,
            label: new_row.label,
            client_type: new_row.client_type,
            username: new_row.username,
            password_hash: new_row.password_hash,
            created_at_ms: new_row.created_at_ms,
            last_seen_at_ms: new_row.last_seen_at_ms,
            last_seen_ip: new_row.last_seen_ip,
            reported_name: new_row.reported_name,
            reported_os: new_row.reported_os,
        };
        let restored = mapper.to_domain(&row).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn unknown_client_type_is_row_corruption() {
        let mapper = MobileDeviceRowMapper;
        let row = MobileDeviceRow {
            device_id: "did_x".into(),
            label: "x".into(),
            client_type: "android_secret".into(),
            username: "mobile_xx".into(),
            password_hash: "$argon2id$...".into(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let err = mapper.to_domain(&row).unwrap_err();
        assert!(err.to_string().contains("unknown client_type"));
    }
}
