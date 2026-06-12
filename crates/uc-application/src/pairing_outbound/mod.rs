//! Joiner-side pairing internals.
//!
//! Symmetric to [`crate::pairing_inbound`] on the sponsor side: wire and
//! crypto work is owned by a coordinator, persistence / setup-status /
//! composition lives in the outer use case
//! ([`crate::usecases::pairing::redeem_invitation::RedeemPairingInvitationUseCase`]).
//!
//! Per `uc-application/AGENTS.md` §11.4 everything here is `pub(crate)`;
//! external callers reach joiner pairing exclusively through
//! [`SpaceSetupFacade::redeem_pairing_invitation`].
//!
//! [`SpaceSetupFacade::redeem_pairing_invitation`]:
//!     crate::facade::space_setup::SpaceSetupFacade::redeem_pairing_invitation

pub(crate) mod joiner_handshake;
