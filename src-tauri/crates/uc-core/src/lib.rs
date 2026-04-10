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
pub mod device;
pub mod ids;
pub mod network;
pub mod ports;
pub mod search;
pub mod security;
pub mod settings;
pub mod setup;

// Re-export commonly used types at the crate root
pub use blob::Blob;
pub use clipboard::*;
pub use config::AppConfig;
pub use device::{Device, DeviceId, DeviceName, DeviceStatus, Platform};
pub use ids::BlobId;
pub use ids::{PeerId, SessionId};
pub use network::{NetworkEvent, NetworkStatus, ProtocolMessage};

// 不知道如何分类，临时定义在这里

pub struct EncryptionMeta {
    pub algo: String,   // "xchacha20poly1305"
    pub key_id: String, // keyslot id / key version
    pub nonce_b64: String,
    pub aad_b64: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MaterializedPayload {
    /// 直接可交付
    Inline {
        mime: Option<String>,
        bytes: Vec<u8>,
    },

    /// 已经落 blob，可交付 blob 引用
    Blob {
        mime: Option<String>,
        blob_id: BlobId,
    },
}
