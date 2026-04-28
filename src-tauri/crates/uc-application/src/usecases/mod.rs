//! Cross-domain application use cases that the future `AppFacade` (P4)
//! composes into user-facing actions.
//!
//! Modules are organised by **primary semantic domain**, not by the ports
//! each use case happens to touch — a single use case (e.g. A1
//! `InitializeSpaceUseCase`) can drive many ports, but conceptually belongs
//! to one domain (setup). Per `uc-application/AGENTS.md` §11.4 every type
//! here stays `pub(crate)`: external crates reach them exclusively through
//! `AppFacade`.

pub(crate) mod blob_transfer;
pub(crate) mod clipboard_history;
pub(crate) mod clipboard_restore;
/// `pub` (not `pub(crate)`) only because Slice 2 Phase 3 · T10 needs a
/// publicly-reachable path to `payload_codec::decode_v3_bytes_to_snapshot`
/// for the CLI `watch` command. Items inside keep their `pub(crate)` caps
/// so the surface stays narrow.
pub mod clipboard_sync;
pub(crate) mod pairing;
pub(crate) mod presence;
pub(crate) mod search;
pub(crate) mod setup;
pub(crate) mod upgrade;
