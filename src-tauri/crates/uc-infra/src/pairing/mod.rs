//! Iroh-native pairing infrastructure.
//!
//! * [`wire`] — binary codec for [`PairingSessionMessage`] using postcard.
//! * [`session`] — `IrohPairingSessionAdapter` implementing the joiner
//!   `dial_by_invitation` flow and the sponsor accept handler on top of
//!   an iroh `Endpoint`.
//! * [`code_mint`] — sponsor-side local code generation (rendezvous no
//!   longer mints).
//! * [`discovery_constants`] — shared mDNS service name and TXT field
//!   keys; keeping these in one module prevents publisher/resolver drift.
//! * [`mdns_publisher`] / [`mdns_resolver`] — window-scoped LAN discovery
//!   channel for the invitation code. Cohabits with the cloud channel
//!   (`crate::rendezvous`) so first-pair-no-WAN can succeed without
//!   forcing the user to flip any setting.
//!
//! [`PairingSessionMessage`]: uc_core::pairing::PairingSessionMessage

pub mod code_mint;
pub mod discovery_constants;
pub mod mdns_publisher;
pub mod mdns_resolver;
pub mod session;
pub mod wire;

pub use code_mint::mint_invitation_code;
pub use mdns_publisher::{MdnsPairingPublisher, MdnsPublisherError, PublisherHandle};
pub use mdns_resolver::{MdnsPairingResolver, MdnsResolverError};
pub use session::{IrohPairingSessionAdapter, PAIRING_ALPN};
pub use wire::{decode, encode, WireDecodeError, WireEncodeError};
