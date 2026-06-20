//! iroh network adapter (Slice 1+).
//!
//! Groups adapters backed by the `iroh` crate: long-term device identity,
//! endpoint lifecycle, session opener, blob transfer. Slice 1 only ships
//! [`IrohIdentityStore`]; later slices add the rest.

pub mod active_clipboard;
mod addr_filter;
pub mod blobs;
pub mod clipboard_dispatch_adapter;
pub mod clipboard_receiver_adapter;
pub mod clipboard_wire;
mod connect;
pub mod connection_channel_adapter;
pub mod identity_store;
pub mod node;
pub mod persistable_addr;
pub mod presence_adapter;
pub mod relay_probe;
pub(crate) mod runtime_consts;
pub mod transfer_progress_adapter;
pub mod transfer_progress_wire;

pub use active_clipboard::{
    IrohActiveClipboardDispatchAdapter, IrohActiveClipboardPullClientAdapter,
    IrohActiveClipboardPullServeAdapter, IrohActiveClipboardPullServeHandler,
    IrohActiveClipboardReceiverAdapter, IrohActiveClipboardReceiverHandler, ACTIVE_CLIPBOARD_ALPN,
    ACTIVE_CLIPBOARD_PULL_ALPN,
};
pub(crate) use addr_filter::filter_endpoint_addr;
pub use blobs::{IrohBlobTransferAdapter, BLOBS_ALPN};
pub use clipboard_dispatch_adapter::{IrohClipboardDispatchAdapter, CLIPBOARD_ALPN};
pub use clipboard_receiver_adapter::{IrohClipboardReceiverAdapter, IrohClipboardReceiverHandler};
pub(crate) use connect::connect_with_staggered_retry;
pub use connection_channel_adapter::IrohConnectionChannelAdapter;
pub use identity_store::{IrohIdentityStore, IDENTITY_STORE_KEY};
pub use node::{
    ActiveClipboardHandlers, ActiveClipboardPullHandlers, BlobHandlers, ClipboardHandlers,
    IrohNode, IrohNodeBuilder, IrohNodeConfig, IrohNodeError, PairingHandlers,
    TransferProgressHandlers,
};
pub use presence_adapter::{IrohPresenceAdapter, IrohPresenceHandler, PRESENCE_ALPN};
pub use relay_probe::{
    IrohRelayProbeAdapter, RelayProbeError as IrohRelayProbeError,
    RelayProbeReport as IrohRelayProbeReport,
};
