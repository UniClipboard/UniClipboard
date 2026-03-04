//! # Platform Adapters / 平台适配器
//!
//! This module contains platform-specific implementations for ports.
//! 此模块包含端口的各种平台特定实现。
//!
//! # Modules / 模块
//!
//! - `blob_store` - Filesystem-based blob storage (implemented)
//! - `blob` - Placeholder blob materializer (to be replaced)
//! - `autostart` - Placeholder autostart management
//! - `clipboard` - Placeholder clipboard materialization
//! - `encryption` - Placeholder encryption session management
//! - `network` - Placeholder P2P networking
//! - `ui` - Placeholder UI operations

pub mod autostart;
pub mod blob;
pub mod blob_store;
pub mod encryption;
pub mod in_memory_watcher_control;
pub mod libp2p_network;
pub mod network;
pub mod pairing_stream;
pub mod ui;

pub use autostart::PlaceholderAutostartPort;
pub use blob::PlaceholderBlobWriterPort;
pub use blob_store::FilesystemBlobStore;
pub use encryption::{InMemoryEncryptionSessionPort, PlaceholderEncryptionSessionPort};
pub use in_memory_watcher_control::InMemoryWatcherControl;
pub use libp2p_network::Libp2pNetworkAdapter;
pub use network::PlaceholderNetworkPort;
pub use ui::PlaceholderUiPort;
