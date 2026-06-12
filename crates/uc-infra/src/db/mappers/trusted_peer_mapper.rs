use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};

use uc_core::security::IdentityFingerprint;
use uc_core::{DeviceId, TrustedPeer};

use crate::db::models::{NewTrustedPeerRow, TrustedPeerRow};
use crate::db::ports::{InsertMapper, RowMapper};

pub struct TrustedPeerRowMapper;

impl InsertMapper<TrustedPeer, NewTrustedPeerRow> for TrustedPeerRowMapper {
    fn to_row(&self, domain: &TrustedPeer) -> Result<NewTrustedPeerRow> {
        Ok(NewTrustedPeerRow {
            peer_device_id: domain.peer_device_id.as_str().to_string(),
            local_device_id: domain.local_device_id.as_str().to_string(),
            peer_fingerprint: domain.peer_fingerprint.as_raw(),
            trusted_at: domain.trusted_at.timestamp(),
        })
    }
}

impl RowMapper<TrustedPeerRow, TrustedPeer> for TrustedPeerRowMapper {
    fn to_domain(&self, row: &TrustedPeerRow) -> Result<TrustedPeer> {
        let trusted_at = Utc
            .timestamp_opt(row.trusted_at, 0)
            .single()
            .ok_or_else(|| anyhow!("invalid trusted_at timestamp: {}", row.trusted_at))?;

        let peer_fingerprint = IdentityFingerprint::from_display_string(&row.peer_fingerprint)
            .map_err(|e| anyhow!("invalid peer_fingerprint in row: {}", e))?;

        Ok(TrustedPeer {
            local_device_id: DeviceId::new(row.local_device_id.clone()),
            peer_device_id: DeviceId::new(row.peer_device_id.clone()),
            peer_fingerprint,
            trusted_at,
        })
    }
}
