// P2P networking module using libp2p
// Replaces WebSocket-based device discovery and clipboard sync

pub mod behaviour;
pub mod blob;
pub mod codec;
pub mod events;
pub mod pairing;
pub mod pin_hash;
pub mod protocol;
pub mod swarm;
pub mod transport;

pub use codec::{PairingRequest as ReqPairingRequest, PairingResponse as ReqPairingResponse};
pub use events::{ConnectedPeer, DiscoveredPeer, NetworkEvent, NetworkStatus};
pub use pairing::{PairingManager, PairingSession};
pub use pin_hash::{hash_pin, verify_pin};
pub use protocol::{ClipboardMessage, PairingRequest, ProtocolMessage};
pub use swarm::{NetworkCommand, NetworkManager};
