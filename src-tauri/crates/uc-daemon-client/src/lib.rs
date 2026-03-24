//! # uc-daemon-client
//!
//! Daemon HTTP and WebSocket client for UniClipboard.
//! Zero Tauri dependencies -- usable from any async context.

pub mod connection;
pub mod http;
pub mod realtime;
pub mod ws_bridge;

pub use connection::DaemonConnectionState;
pub use http::{
    DaemonPairingClient, DaemonPairingRequestError, DaemonQueryClient, DaemonSetupClient,
};
pub use realtime::{install_daemon_setup_pairing_facade, start_realtime_runtime};
pub use ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, DaemonWsBridgeError};
