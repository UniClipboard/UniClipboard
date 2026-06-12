//! Application-internal holder for outstanding sponsor-side pairing
//! invitations.
//!
//! Sits outside `crate::pairing` (which is the pre-Slice-1 libp2p pairing
//! stack) so the new Slice 1 invitation flow doesn't pollute the legacy
//! namespace on the way to its eventual removal (Slice 5).
//!
//! All types here are `pub(crate)` per `uc-application/AGENTS.md` §11.4:
//! the holder is a cross-use-case flow-state component, not an external
//! boundary. External callers interact with invitations exclusively
//! through [`crate::facade::space_setup::SpaceSetupFacade`].

pub(crate) mod holder;

pub(crate) use holder::InMemoryPairingInvitationHolder;
