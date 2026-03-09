//! Clipboard domain models.
mod change;
mod decision;
mod entry;
mod event;
mod hash;
mod mime;
mod origin;
mod payload_availability;
mod policy;
mod selection;
mod snapshot;
mod system;
mod thumbnail;
mod timestamp;

pub use change::*;
pub use entry::*;
pub use event::*;
pub use policy::ClipboardSelection;
pub use policy::*;
pub use selection::*;
pub use snapshot::*;
pub use system::{
    ObservedClipboardRepresentation, RepresentationHash, SnapshotHash, SystemClipboardSnapshot,
};

pub use decision::{ClipboardContentActionDecision, DuplicationHint, RejectReason};
pub use hash::{ContentHash, HashAlgorithm};
pub use mime::MimeType;
pub use origin::ClipboardOrigin;
pub use payload_availability::PayloadAvailability;
pub use thumbnail::ThumbnailMetadata;
pub use timestamp::TimestampMs;
