//! Offline-first space member removal.

use crate::commands::app_session::connect_facade_with_lease;
use crate::exit_codes;
use crate::{output, ui};
use uc_daemon_contract::api::dto::member::{
    DeviceGroupRelationshipDto, DeviceMembershipDto, DeviceTrustSnapshotDto,
};

pub async fn remove(peer_id: String, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Member removal");
    }

    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let device_trust = match service.remove_member(peer_id.clone()).await {
        Ok(device_trust) => device_trust,
        Err(error) => {
            ui::error(&format!(
                "Failed to remove member: {}",
                crate::commands::daemon_error_message(&error)
            ));
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        output::emit_json(&device_trust, "device trust")
    } else {
        ui::success("Member removed from this device.");
        if removal_notification_pending(&device_trust, &peer_id) {
            ui::info("delivery", "notifying other devices");
        }
        ui::info("revision", &device_trust.revision.to_string());
        exit_codes::EXIT_SUCCESS
    }
}

fn removal_notification_pending(snapshot: &DeviceTrustSnapshotDto, peer_id: &str) -> bool {
    snapshot.devices.iter().any(|device| {
        device.device_id == peer_id
            && device.membership == DeviceMembershipDto::Removed
            && device.group_relationship
                == DeviceGroupRelationshipDto::AwaitingRemovalAcknowledgement
    })
}

#[cfg(test)]
mod tests {
    use super::removal_notification_pending;
    use uc_daemon_contract::api::dto::member::{
        DeviceCompatibilityDto, DeviceGroupRelationshipDto, DeviceMembershipDto,
        DeviceReachabilityDto, DeviceSyncRelationshipDto, DeviceTrustRelationshipDto,
        DeviceTrustSnapshotDto,
    };

    fn snapshot(group_relationship: DeviceGroupRelationshipDto) -> DeviceTrustSnapshotDto {
        DeviceTrustSnapshotDto {
            revision: 3,
            local_device_id: "local".into(),
            local_membership: DeviceMembershipDto::Active,
            current_change: None,
            current_join: None,
            pending_inbound_member: None,
            inbound_pairings: vec![],
            space_device_update: Default::default(),
            maintenance_health: Default::default(),
            devices: vec![DeviceTrustRelationshipDto {
                device_id: "peer".into(),
                display_name: "Peer".into(),
                is_local: false,
                reachability: DeviceReachabilityDto::Offline,
                membership: DeviceMembershipDto::Removed,
                group_relationship,
                compatibility: DeviceCompatibilityDto::Compatible,
                sync_relationship: DeviceSyncRelationshipDto::RemovedPeerDevice,
                pairing_confirmation: None,
                available_actions: vec![],
                blocked_reason: None,
            }],
            recovery: "not_available_in_this_version".into(),
            allowed_actions: vec![],
            blocked_reason: None,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn identifies_engine_owned_removal_delivery_without_inferring_it() {
        assert!(removal_notification_pending(
            &snapshot(DeviceGroupRelationshipDto::AwaitingRemovalAcknowledgement),
            "peer"
        ));
        assert!(!removal_notification_pending(
            &snapshot(DeviceGroupRelationshipDto::Unknown),
            "peer"
        ));
    }
}
