use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::DeviceId;
use crate::security::IdentityFingerprint;

use super::preferences::MemberSyncPreferences;

/// A device admitted as a member of the local space.
///
/// Revocation is modelled as outright removal from the repository rather
/// than a state transition, so a `SpaceMember` record always represents an
/// active member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceMember {
    pub device_id: DeviceId,
    pub device_name: String,
    pub identity_fingerprint: IdentityFingerprint,
    pub joined_at: DateTime<Utc>,
    pub sync_preferences: MemberSyncPreferences,
}
