//! 宿主事件入口。

mod composite;
mod event;
mod outbound_entry_cache;
mod publisher;

pub use composite::CompositeHostEventEmitter;
pub use event::{
    ClipboardHostEvent, ClipboardOriginKind, DeliveryHostEvent, DeliveryStatusKind, EmitError,
    HostEvent, HostEventEmitterPort, TransferHostEvent,
};
pub use outbound_entry_cache::OutboundEntryIdCache;
pub use publisher::FileTransferHostEventPublisher;
