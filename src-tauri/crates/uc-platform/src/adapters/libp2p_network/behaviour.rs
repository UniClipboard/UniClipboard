use anyhow::{anyhow, Result};
use libp2p::{mdns, swarm::NetworkBehaviour, PeerId};
use libp2p_stream as stream;
use std::time::Duration;

use super::{START_STATE_FAILED, START_STATE_IDLE, START_STATE_STARTED, START_STATE_STARTING};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Libp2pBehaviourEvent")]
pub(crate) struct Libp2pBehaviour {
    pub(crate) mdns: mdns::tokio::Behaviour,
    pub(crate) stream: stream::Behaviour,
}

#[derive(Debug)]
pub(crate) enum Libp2pBehaviourEvent {
    Mdns(mdns::Event),
    Stream,
}

impl From<mdns::Event> for Libp2pBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        Self::Mdns(event)
    }
}

impl From<()> for Libp2pBehaviourEvent {
    fn from(_: ()) -> Self {
        Self::Stream
    }
}

pub(crate) fn build_mdns_config() -> mdns::Config {
    let mut config = mdns::Config::default();
    config.query_interval = Duration::from_secs(5);
    config
}

pub(crate) fn start_state_name(state: u8) -> &'static str {
    match state {
        START_STATE_IDLE => "idle",
        START_STATE_STARTING => "starting",
        START_STATE_STARTED => "started",
        START_STATE_FAILED => "failed",
        _ => "unknown",
    }
}

impl Libp2pBehaviour {
    pub(crate) fn new(local_peer_id: PeerId) -> Result<Self> {
        let mdns = mdns::tokio::Behaviour::new(build_mdns_config(), local_peer_id)
            .map_err(|e| anyhow!("failed to create mdns behaviour: {e}"))?;
        let stream = stream::Behaviour::new();
        Ok(Self { mdns, stream })
    }
}
