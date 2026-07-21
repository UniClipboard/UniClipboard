use uc_application::facade::{AppFacade, RosterError};
use uc_core::ports::ConnectionChannel;

use crate::{
    EngineError, EngineErrorCategory, OperationResult, PeerConnectionChannelSummary,
    PeerConnectionRefreshSummary, PeerConnectionSummary,
};

pub const QUERY_PEER_CONNECTIONS_FAILED_CODE: u32 = 1381;
pub const REFRESH_PEER_CONNECTIONS_FAILED_CODE: u32 = 1382;

pub(crate) async fn execute_query_peer_connections(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let peers = facade
        .list_peer_snapshots()
        .await
        .map_err(map_query_error)?;
    Ok(OperationResult::PeerConnections(
        peers
            .into_iter()
            .map(|peer| PeerConnectionSummary {
                peer_id: peer.peer_id,
                device_name: peer.device_name,
                addresses: peer.addresses,
                is_paired: peer.is_paired,
                connected: peer.connected,
                pairing_state: peer.pairing_state,
                channel: map_channel(peer.channel),
                connection_address: peer.connection_address,
            })
            .collect(),
    ))
}

pub(crate) async fn execute_refresh_peer_connections(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let report = facade.refresh_presence().await.map_err(|_| {
        EngineError::new(
            REFRESH_PEER_CONNECTIONS_FAILED_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    Ok(OperationResult::PeerConnectionsRefreshed(
        PeerConnectionRefreshSummary {
            total: report.total as u32,
            online: report.online as u32,
            offline: report.offline as u32,
            errors: report.errors.len() as u32,
        },
    ))
}

fn map_channel(channel: ConnectionChannel) -> PeerConnectionChannelSummary {
    match channel {
        ConnectionChannel::Direct => PeerConnectionChannelSummary::Direct,
        ConnectionChannel::Relay => PeerConnectionChannelSummary::Relay,
        ConnectionChannel::Offline => PeerConnectionChannelSummary::Offline,
        ConnectionChannel::Unknown => PeerConnectionChannelSummary::Unknown,
    }
}

fn map_query_error(error: RosterError) -> EngineError {
    let (category, retryable) = match error {
        RosterError::Unavailable => (EngineErrorCategory::Unavailable, true),
        _ => (EngineErrorCategory::Internal, false),
    };
    EngineError::new(QUERY_PEER_CONNECTIONS_FAILED_CODE, category, retryable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_channels_have_a_total_stable_mapping() {
        assert_eq!(
            map_channel(ConnectionChannel::Direct),
            PeerConnectionChannelSummary::Direct
        );
        assert_eq!(
            map_channel(ConnectionChannel::Relay),
            PeerConnectionChannelSummary::Relay
        );
        assert_eq!(
            map_channel(ConnectionChannel::Offline),
            PeerConnectionChannelSummary::Offline
        );
        assert_eq!(
            map_channel(ConnectionChannel::Unknown),
            PeerConnectionChannelSummary::Unknown
        );
    }

    #[test]
    fn query_errors_do_not_expose_internal_details() {
        let error = map_query_error(RosterError::MemberRepository(
            "private database path".to_string(),
        ));

        assert_eq!(error.code(), QUERY_PEER_CONNECTIONS_FAILED_CODE);
        assert_eq!(error.category(), EngineErrorCategory::Internal);
        assert!(!format!("{error:?}").contains("private database path"));
    }
}
