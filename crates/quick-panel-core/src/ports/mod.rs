//! Interfaces the panel needs from the outside world.
//!
//! The app crate implements them: the operating system behind [`platform`], the daemon behind
//! [`history`]. The state machine only sees these traits, so tests can replace them.

pub mod history;
pub mod host;
pub mod platform;

pub use history::{
    EntryAction, HistoryService, ImagePayload, Live, Options, PreviewData, SearchFailure,
    ServiceError,
};
pub use host::{HostLink, HostRequest};
pub use platform::{Capabilities, PasteTarget, PlatformError};
