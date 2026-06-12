use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::DeviceId;
use crate::security::IdentityFingerprint;

/// Aggregate root for a persisted "this peer is trusted to communicate with us" fact.
///
/// Distrust is modelled as outright removal from the repository (hard-delete),
/// so a `TrustedPeer` record always represents an active trust relationship.
/// Display name, sync preferences and reachability state belong to other
/// domains (`membership`, `network`) and are deliberately absent here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub local_device_id: DeviceId,
    pub peer_device_id: DeviceId,
    pub peer_fingerprint: IdentityFingerprint,
    pub trusted_at: DateTime<Utc>,
}
