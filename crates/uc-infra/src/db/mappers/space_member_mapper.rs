use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};

use uc_core::security::IdentityFingerprint;
use uc_core::{DeviceId, MemberSyncPreferences, SpaceMember};

use crate::db::models::{NewSpaceMemberRow, SpaceMemberRow};
use crate::db::ports::{InsertMapper, RowMapper};

pub struct SpaceMemberRowMapper;

impl InsertMapper<SpaceMember, NewSpaceMemberRow> for SpaceMemberRowMapper {
    fn to_row(&self, domain: &SpaceMember) -> Result<NewSpaceMemberRow> {
        let sync_preferences_json = serde_json::to_string(&domain.sync_preferences)
            .map_err(|e| anyhow!("serialize sync_preferences: {}", e))?;

        Ok(NewSpaceMemberRow {
            device_id: domain.device_id.as_str().to_string(),
            device_name: domain.device_name.clone(),
            identity_fingerprint: domain.identity_fingerprint.as_raw(),
            joined_at: domain.joined_at.timestamp(),
            sync_preferences: sync_preferences_json,
        })
    }
}

impl RowMapper<SpaceMemberRow, SpaceMember> for SpaceMemberRowMapper {
    fn to_domain(&self, row: &SpaceMemberRow) -> Result<SpaceMember> {
        let joined_at = Utc
            .timestamp_opt(row.joined_at, 0)
            .single()
            .ok_or_else(|| anyhow!("invalid joined_at timestamp: {}", row.joined_at))?;

        let sync_preferences: MemberSyncPreferences =
            serde_json::from_str(&row.sync_preferences)
                .map_err(|e| anyhow!("deserialize sync_preferences: {}", e))?;

        let identity_fingerprint =
            IdentityFingerprint::from_display_string(&row.identity_fingerprint)
                .map_err(|e| anyhow!("invalid identity_fingerprint in row: {}", e))?;

        Ok(SpaceMember {
            device_id: DeviceId::new(row.device_id.clone()),
            device_name: row.device_name.clone(),
            identity_fingerprint,
            joined_at,
            sync_preferences,
        })
    }
}
