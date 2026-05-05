//! `MobileDeviceRowMapper` —— `MobileDevice` ↔ sqlite 行的边界转换。
//!
//! token_hash 在 schema 上是 BLOB(Diesel `Vec<u8>`),domain 上是 `[u8; 32]`
//! 包装。映射时严格校验长度:不是 32 字节即视为行损坏(理论上 UNIQUE BLOB
//! 约束 + minter 行为已经保证写入侧合法,这里只是兜底)。
//!
//! `client_type` 走 `MobileClientType::as_wire_str` / `from_wire_str` 这对
//! 函数,陌生值同样按行损坏处理。

use anyhow::{anyhow, Result};

use uc_core::mobile_sync::{MobileClientType, MobileDevice, MobileDeviceId, TokenHash};

use crate::db::models::{MobileDeviceRow, NewMobileDeviceRow};
use crate::db::ports::{InsertMapper, RowMapper};

pub struct MobileDeviceRowMapper;

impl InsertMapper<MobileDevice, NewMobileDeviceRow> for MobileDeviceRowMapper {
    fn to_row(&self, domain: &MobileDevice) -> Result<NewMobileDeviceRow> {
        Ok(NewMobileDeviceRow {
            device_id: domain.device_id.as_str().to_string(),
            label: domain.label.clone(),
            client_type: domain.client_type.as_wire_str().to_string(),
            token_hash: domain.token_hash.as_bytes().to_vec(),
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

        let token_hash_arr: [u8; 32] = row.token_hash.as_slice().try_into().map_err(|_| {
            anyhow!(
                "mobile_device row {} has token_hash of length {}, expected 32",
                row.device_id,
                row.token_hash.len()
            )
        })?;

        Ok(MobileDevice {
            device_id: MobileDeviceId::new(row.device_id.clone()),
            label: row.label.clone(),
            client_type,
            token_hash: TokenHash::new(token_hash_arr),
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

    fn fixture(token_byte: u8) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new("did_abc"),
            label: "iPhone".to_string(),
            client_type: MobileClientType::IosShortcut,
            token_hash: TokenHash::new([token_byte; 32]),
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
        let original = fixture(7);

        let new_row = mapper.to_row(&original).expect("to_row");
        // 模拟从 sqlite 读回:NewMobileDeviceRow → MobileDeviceRow 字段同结构。
        let row = MobileDeviceRow {
            device_id: new_row.device_id,
            label: new_row.label,
            client_type: new_row.client_type,
            token_hash: new_row.token_hash,
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
            token_hash: TokenHash::new([1; 32]),
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
            token_hash: new_row.token_hash,
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
            token_hash: vec![0; 32],
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let err = mapper.to_domain(&row).unwrap_err();
        assert!(err.to_string().contains("unknown client_type"));
    }

    #[test]
    fn token_hash_wrong_length_is_row_corruption() {
        let mapper = MobileDeviceRowMapper;
        let row = MobileDeviceRow {
            device_id: "did_x".into(),
            label: "x".into(),
            client_type: "ios_shortcut".into(),
            token_hash: vec![0; 16], // wrong length
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let err = mapper.to_domain(&row).unwrap_err();
        assert!(err.to_string().contains("expected 32"));
    }
}
