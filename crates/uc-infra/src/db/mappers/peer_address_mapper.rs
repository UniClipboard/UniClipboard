use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};

use uc_core::ids::DeviceId;
use uc_core::ports::PeerAddressRecord;

use crate::db::models::{NewPeerAddressRow, PeerAddressRow};
use crate::db::ports::{InsertMapper, RowMapper};

pub struct PeerAddressRowMapper;

impl InsertMapper<PeerAddressRecord, NewPeerAddressRow> for PeerAddressRowMapper {
    fn to_row(&self, domain: &PeerAddressRecord) -> Result<NewPeerAddressRow> {
        Ok(NewPeerAddressRow {
            device_id: domain.device_id.as_str().to_string(),
            addr_blob: domain.addr_blob.clone(),
            observed_at: domain.observed_at.timestamp(),
        })
    }
}

impl RowMapper<PeerAddressRow, PeerAddressRecord> for PeerAddressRowMapper {
    fn to_domain(&self, row: &PeerAddressRow) -> Result<PeerAddressRecord> {
        let observed_at = Utc
            .timestamp_opt(row.observed_at, 0)
            .single()
            .ok_or_else(|| anyhow!("invalid observed_at timestamp: {}", row.observed_at))?;

        Ok(PeerAddressRecord {
            device_id: DeviceId::new(row.device_id.clone()),
            addr_blob: row.addr_blob.clone(),
            observed_at,
        })
    }
}
