//! iroh adapters for the active-clipboard subsystem.
//!
//! Groups the dispatch / receiver / pull adapters and their wire codecs
//! so the active-clipboard transport stays together instead of being
//! flattened across `network/iroh` with an `active_clipboard_` name
//! prefix.

pub mod dispatch_adapter;
pub mod pull_client_adapter;
pub mod pull_serve_adapter;
pub mod pull_wire;
pub mod receiver_adapter;
pub mod wire;

pub use dispatch_adapter::IrohActiveClipboardDispatchAdapter;
pub use pull_client_adapter::IrohActiveClipboardPullClientAdapter;
pub use pull_serve_adapter::{
    IrohActiveClipboardPullServeAdapter, IrohActiveClipboardPullServeHandler,
    ACTIVE_CLIPBOARD_PULL_ALPN,
};
pub use receiver_adapter::{
    IrohActiveClipboardReceiverAdapter, IrohActiveClipboardReceiverHandler, ACTIVE_CLIPBOARD_ALPN,
};
