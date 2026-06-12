//! Sponsor / joiner pairing use cases for the Slice 1 iroh-native flow.
//!
//! B1 · [`IssuePairingInvitationUseCase`] — sponsor asks the rendezvous
//! service for a fresh code, materialises a [`PairingInvitation`]
//! aggregate, and parks it in the in-memory holder for the subsequent
//! (P7e) `Incoming` event to match against.
//!
//! [`IssuePairingInvitationUseCase`]: issue_invitation::IssuePairingInvitationUseCase
//! [`PairingInvitation`]: uc_core::pairing::invitation::PairingInvitation

pub(crate) mod issue_invitation;
pub(crate) mod redeem_invitation;
