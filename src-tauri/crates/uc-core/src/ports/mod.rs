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

pub mod app_version;
pub mod blob;
pub mod cache_fs;
pub mod clipboard;
mod clipboard_change_handler;
mod clipboard_event;
mod clock;
pub mod connection_channel;
pub mod device_identity;
pub mod errors;
pub mod file_transfer_repository;
mod hash;
pub mod local_identity;
pub mod pairing;
pub mod pairing_invitation;
pub mod peer_address;
pub mod presence;
pub mod search;
pub mod security;
pub mod settings;
pub mod setup;
pub mod space;
mod timer;

pub use app_version::{AppVersionStateError, AppVersionStatePort};
pub use cache_fs::{CacheFsPort, DirEntry as CacheFsDirEntry};
pub use clipboard_event::*;
pub use clock::*;
pub use hash::*;
pub use timer::TimerPort;

pub use clipboard::*;
pub use clipboard_change_handler::ClipboardChangeHandler;
pub use connection_channel::{ConnectionChannel, ConnectionChannelPort};
pub use device_identity::DeviceIdentityPort;
pub use errors::AppDirsError;
pub use file_transfer_repository::{
    compute_aggregate_status, EntryTransferSummary, ExpiredInflightTransfer,
    FileTransferRepositoryPort, NoopFileTransferRepositoryPort, PendingInboundTransfer,
    TrackedFileTransfer, TrackedFileTransferStatus,
};
pub use local_identity::{LocalIdentityError, LocalIdentityPort};
pub use pairing::{
    DialError, PairingEventPort, PairingSessionEvent, PairingSessionId, PairingSessionPort,
    SessionError,
};
pub use pairing_invitation::{
    ConsumeInvitationError, InvitationCode, InvitationError, IssuedInvitation,
    PairingInvitationPort,
};
pub use peer_address::{PeerAddressError, PeerAddressRecord, PeerAddressRepositoryPort};
pub use presence::{PresenceError, PresenceEvent, PresencePort, ReachabilityState};
pub use search::search_index::SearchIndexPort;
pub use search::search_key::SearchKeyDerivationPort;
pub use security::secure_storage::{SecureStorageError, SecureStoragePort};
pub use security::transfer_cipher::{TransferCipherError, TransferCipherPort};
pub use security::{BlobCipherError, BlobCipherPort};
pub use settings::{SettingsMigrationPort, SettingsPort};
pub use setup::SetupStatusPort;
