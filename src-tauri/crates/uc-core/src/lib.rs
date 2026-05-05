//! # uc-core
//!
//! Core domain models and business logic for UniClipboard.
//!
//! This crate contains pure business logic without any infrastructure dependencies.

// Public module exports
pub mod app_dirs;
pub mod blob;
pub mod clipboard;
pub mod config;
pub mod crypto;
pub mod file_transfer;
pub mod ids;
pub mod membership;
pub mod mobile_sync;
pub mod network;
pub mod pairing;
pub mod ports;
pub mod search;
pub mod security;
pub mod settings;
pub mod setup;
pub mod space_access;
pub mod trusted_peer;

pub use membership::{MemberRepositoryPort, MemberSyncPreferences, MembershipError, SpaceMember};
pub use security::{FingerprintError, IdentityFingerprint};
pub use trusted_peer::{
    TrustAbortReason, TrustedPeer, TrustedPeerError, TrustedPeerEvent, TrustedPeerRepositoryPort,
};
// Re-export commonly used types at the crate root
pub use clipboard::*;
pub use config::AppConfig;
pub use file_transfer::{
    FileTransferCancellationReason, FileTransferDirection, FileTransferEvent,
    FileTransferFailureReason, FileTransferProgress,
};
pub use ids::{BlobId, DeviceId, SessionId};
