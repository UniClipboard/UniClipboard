use crate::settings::model::SyncSettings;
use crate::PeerId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingState {
    Pending,
    Trusted,
    Revoked,
}

impl fmt::Display for PairingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            PairingState::Pending => "Pending",
            PairingState::Trusted => "Trusted",
            PairingState::Revoked => "Revoked",
        };
        f.write_str(label)
    }
}

impl FromStr for PairingState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(PairingState::Pending),
            "Trusted" => Ok(PairingState::Trusted),
            "Revoked" => Ok(PairingState::Revoked),
            _ => Err(format!("invalid PairingState: {s}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub peer_id: PeerId,
    pub pairing_state: PairingState,
    pub identity_fingerprint: String,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub device_name: String,
    #[serde(default)]
    pub sync_settings: Option<SyncSettings>,
}

/// Returns the effective sync settings for a device.
///
/// If the device has per-device overrides, those are used; otherwise the
/// global defaults are returned.
pub fn resolve_sync_settings<'a>(
    device: &'a PairedDevice,
    global: &'a SyncSettings,
) -> &'a SyncSettings {
    device.sync_settings.as_ref().unwrap_or(global)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paired_device_serialization() {
        let device = PairedDevice {
            peer_id: PeerId::from("12D3KooW..."),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "Test Device".to_string(),
            sync_settings: None,
        };

        let json = serde_json::to_string(&device).unwrap();
        let restored: PairedDevice = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.pairing_state, PairingState::Trusted);
        assert_eq!(restored.identity_fingerprint, device.identity_fingerprint);
        assert_eq!(restored.device_name, device.device_name);
        assert!(restored.sync_settings.is_none());
    }

    #[test]
    fn test_deserialize_without_sync_settings_defaults_to_none() {
        let json = r#"{
            "peer_id": "12D3KooW...",
            "pairing_state": "Pending",
            "identity_fingerprint": "fp",
            "paired_at": "2026-01-01T00:00:00Z",
            "last_seen_at": null,
            "device_name": "Old Device"
        }"#;

        let device: PairedDevice = serde_json::from_str(json).unwrap();
        assert!(device.sync_settings.is_none());
    }

    #[test]
    fn pairing_state_display_is_stable() {
        assert_eq!(PairingState::Pending.to_string(), "Pending");
        assert_eq!(PairingState::Trusted.to_string(), "Trusted");
        assert_eq!(PairingState::Revoked.to_string(), "Revoked");
    }

    #[test]
    fn pairing_state_from_str_parses_stable_values() {
        assert_eq!(
            "Pending".parse::<PairingState>().unwrap(),
            PairingState::Pending
        );
        assert_eq!(
            "Trusted".parse::<PairingState>().unwrap(),
            PairingState::Trusted
        );
        assert_eq!(
            "Revoked".parse::<PairingState>().unwrap(),
            PairingState::Revoked
        );
    }

    #[test]
    fn pairing_state_from_str_rejects_unknown_values() {
        assert!("Unknown".parse::<PairingState>().is_err());
    }

    #[test]
    fn test_resolve_sync_settings_uses_device_override() {
        use crate::settings::model::{ContentTypes, SyncFrequency, SyncSettings};

        let global = SyncSettings {
            auto_sync: true,
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes::default(),
        };

        let device_settings = SyncSettings {
            auto_sync: false,
            sync_frequency: SyncFrequency::Interval,
            content_types: ContentTypes::default(),
        };

        let device = PairedDevice {
            peer_id: PeerId::from("peer-1"),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "Dev".to_string(),
            sync_settings: Some(device_settings),
        };

        let resolved = resolve_sync_settings(&device, &global);
        assert!(!resolved.auto_sync);
        assert_eq!(resolved.sync_frequency, SyncFrequency::Interval);
    }

    #[test]
    fn test_resolve_sync_settings_falls_back_to_global() {
        use crate::settings::model::{ContentTypes, SyncFrequency, SyncSettings};

        let global = SyncSettings {
            auto_sync: true,
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes::default(),
        };

        let device = PairedDevice {
            peer_id: PeerId::from("peer-1"),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "Dev".to_string(),
            sync_settings: None,
        };

        let resolved = resolve_sync_settings(&device, &global);
        assert!(resolved.auto_sync);
        assert_eq!(resolved.sync_frequency, SyncFrequency::Realtime);
    }
}
