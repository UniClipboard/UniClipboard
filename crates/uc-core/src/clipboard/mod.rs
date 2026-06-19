//! Clipboard domain models.
mod active_state;
mod category;
mod change;
mod decision;
mod delivery;
mod entry;
mod event;
mod hash;
pub mod integration_mode;
pub mod link_utils;
mod mime;
mod origin;
mod payload_availability;
mod policy;
mod repository_error;
mod selection;
mod snapshot;
mod system;
mod thumbnail;
mod timestamp;

pub use active_state::ActiveClipboardState;
pub use category::{ClipboardContentCategory, ClipboardContentCategorySet};
pub use change::*;
pub use delivery::{
    DeliveryFailureReason, EntryDeliveryError, EntryDeliveryRecord, EntryDeliveryStatus,
};
pub use entry::*;
pub use event::*;
pub use policy::ClipboardSelection;
pub use policy::*;
pub use repository_error::ClipboardRepositoryError;
pub use selection::*;
pub use snapshot::*;
pub use system::{
    is_file_mime_or_format, is_plain_text_mime_or_format, ClipboardPayloadSource,
    ObservedClipboardRepresentation, RepresentationHash, SnapshotHash, SystemClipboardSnapshot,
};

pub use decision::{ClipboardContentActionDecision, DuplicationHint, RejectReason};
pub use hash::{ContentHash, HashAlgorithm};
pub use integration_mode::ClipboardIntegrationMode;
pub use mime::{normalize_wire_mime, ImageKind, MimeClass, MimeType};
pub use origin::ClipboardOrigin;
pub use payload_availability::PayloadAvailability;
pub use thumbnail::ThumbnailMetadata;
pub use timestamp::TimestampMs;
