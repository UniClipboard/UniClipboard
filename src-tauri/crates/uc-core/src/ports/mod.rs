//! Port interfaces for the application layer
//!
//! Ports define the contract between the application logic (use cases)
//! and infrastructure implementations. This follows Hexagonal Architecture
//! principles, allowing the core business logic to remain independent of
//! external dependencies.
//!
//! ## Port Placement Guidelines
//!
//! Before adding a new port to `uc-core/ports`, ask yourself three questions:
//!
//! 1. **Does this port represent a business capability?**
//! 2. **Will it be depended upon by multiple use cases or domains?**
//! 3. **Is it implemented by the infrastructure or platform layer?**
//!
//! If all three answers are **yes**, place it in `uc-core/ports`.
//! Otherwise, place it in the relevant `domain` submodule.

pub mod app_runtime;
pub mod blob_repository;
mod blob_store;
mod blob_writer;
pub mod clipboard;
mod clipboard_change_handler;
mod clipboard_event;
pub mod clipboard_transport;
mod clock;
pub mod connection_policy;
pub mod device_identity;
pub mod device_repository;
mod discovery;
pub mod errors;
pub mod file_manager;
mod hash;
pub mod network_control;
pub mod network_events;
pub mod paired_device_repository;
pub mod pairing_transport;
pub mod peer_directory;
pub mod security;
pub mod settings;
pub mod setup;
pub mod setup_event_port;
pub mod space;
pub mod start_clipboard_watcher;
mod timer;
pub mod transfer_progress;

pub use blob_repository::BlobRepositoryPort;
pub use blob_store::BlobStorePort;
pub use blob_writer::BlobWriterPort;
pub use clipboard_event::*;
pub use clock::*;
pub use connection_policy::{ConnectionPolicyResolverError, ConnectionPolicyResolverPort};
pub use discovery::DiscoveryPort;
pub use hash::*;
pub use timer::TimerPort;

pub use app_runtime::AppRuntimePort;
pub use clipboard::*;
pub use clipboard_change_handler::ClipboardChangeHandler;
pub use clipboard_transport::ClipboardTransportPort;
pub use device_identity::DeviceIdentityPort;
pub use device_repository::DeviceRepositoryPort;
pub use errors::{AppDirsError, DeviceRepositoryError, PairedDeviceRepositoryError};
pub use file_manager::{FileManagerError, FileManagerPort};
pub use network_control::NetworkControlPort;
pub use network_events::NetworkEventPort;
pub use paired_device_repository::PairedDeviceRepositoryPort;
pub use pairing_transport::PairingTransportPort;
pub use peer_directory::PeerDirectoryPort;
pub use security::encryption::EncryptionPort;
pub use security::encryption_session::EncryptionSessionPort;
pub use security::key_material::KeyMaterialPort;
pub use security::secure_storage::{SecureStorageError, SecureStoragePort};
pub use security::transfer_crypto::{
    TransferCryptoError, TransferPayloadDecryptorPort, TransferPayloadEncryptorPort,
};
pub use settings::{SettingsMigrationPort, SettingsPort};
pub use setup::SetupStatusPort;
pub use setup_event_port::SetupEventPort;
pub use start_clipboard_watcher::{StartClipboardWatcherError, StartClipboardWatcherPort};
pub use transfer_progress::{TransferDirection, TransferProgress, TransferProgressPort};
