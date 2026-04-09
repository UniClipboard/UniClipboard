use crate::db::models::{NewPairedDeviceRow, PairedDeviceRow};
use crate::db::ports::{InsertMapper, RowMapper};
use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use std::str::FromStr;
use uc_core::network::{PairedDevice, PairingState};
use uc_core::PeerId;

pub struct PairedDeviceRowMapper;

impl InsertMapper<PairedDevice, NewPairedDeviceRow> for PairedDeviceRowMapper {
    fn to_row(&self, domain: &PairedDevice) -> Result<NewPairedDeviceRow> {
        let sync_settings_json = domain
            .sync_settings
            .as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()
            .map_err(|e| anyhow!("serialize sync_settings: {}", e))?;

        Ok(NewPairedDeviceRow {
            peer_id: domain.peer_id.as_str().to_string(),
            pairing_state: domain.pairing_state.to_string(),
            identity_fingerprint: domain.identity_fingerprint.clone(),
            paired_at: domain.paired_at.timestamp(),
            last_seen_at: domain.last_seen_at.map(|dt| dt.timestamp()),
            device_name: domain.device_name.clone(),
            sync_settings: sync_settings_json,
        })
    }
}

impl RowMapper<PairedDeviceRow, PairedDevice> for PairedDeviceRowMapper {
    fn to_domain(&self, row: &PairedDeviceRow) -> Result<PairedDevice> {
        let paired_at = timestamp_to_utc(row.paired_at, "paired_at")?;
        let last_seen_at = match row.last_seen_at {
            Some(ts) => Some(timestamp_to_utc(ts, "last_seen_at")?),
            None => None,
        };

        let sync_settings = row
            .sync_settings
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| anyhow!("deserialize sync_settings: {}", e))?;

        Ok(PairedDevice {
            peer_id: PeerId::from(row.peer_id.as_str()),
            pairing_state: PairingState::from_str(&row.pairing_state)
                .map_err(|e| anyhow!("{}", e))?,
            identity_fingerprint: row.identity_fingerprint.clone(),
            paired_at,
            last_seen_at,
            device_name: row.device_name.clone(),
            sync_settings,
        })
    }
}

fn timestamp_to_utc(ts: i64, field: &str) -> Result<chrono::DateTime<Utc>> {
    Utc.timestamp_opt(ts, 0)
        .single()
        .ok_or_else(|| anyhow!("invalid {} timestamp: {}", field, ts))
}
