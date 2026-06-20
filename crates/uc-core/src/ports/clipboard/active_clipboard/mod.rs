//! Active-clipboard ports — the cross-device last-writer-wins clipboard
//! register and its dispatch / pull / receiver companions.
//!
//! Grouped under one module so the four traits that make up the
//! active-clipboard subsystem stay together instead of being flattened
//! across `ports/clipboard` with an `active_clipboard_` name prefix.

mod dispatch;
mod pull;
mod receiver;
mod register;

pub use dispatch::{ActiveClipboardDispatchError, ActiveClipboardDispatchPort};
pub use pull::{
    ActiveClipboardPullClientError, ActiveClipboardPullClientPort, ActiveClipboardPullServeError,
    ActiveClipboardPullServePort,
};
pub use receiver::{ActiveClipboardReceiverPort, InboundActiveClipboardState};
pub use register::{
    ActiveClipboardRegisterError, AdvanceActiveClipboardPort, LoadActiveClipboardPort,
    ResetActiveClipboardPort,
};
