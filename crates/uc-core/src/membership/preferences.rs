use serde::{Deserialize, Serialize};

use crate::settings::model::ContentTypes;

/// Preferences the local device holds toward a specific remote member.
///
/// Membership is **local-authoritative** under the single-space model:
/// each device keeps its own view of how it wants to interact with every
/// other member, and these preferences are never synchronized with peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberSyncPreferences {
    /// Whether to push local clipboard changes to the remote member.
    pub send_enabled: bool,
    /// Whether to accept incoming clipboard changes from the remote member.
    pub receive_enabled: bool,
    /// Content-type filter applied when sending to the remote member.
    pub send_content_types: ContentTypes,
    /// Content-type filter applied when receiving from the remote member.
    pub receive_content_types: ContentTypes,
}

impl Default for MemberSyncPreferences {
    fn default() -> Self {
        Self {
            send_enabled: true,
            receive_enabled: true,
            send_content_types: ContentTypes::default(),
            receive_content_types: ContentTypes::default(),
        }
    }
}
