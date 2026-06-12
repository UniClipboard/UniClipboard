//! Sponsor-side inbound pairing orchestrator (Slice 1 P7e).
//!
//! Bridges [`PairingEventPort::subscribe`] to the in-memory invitation
//! holder and the rendezvous consume path:
//!
//! 1. Subscribe to inbound pairing events.
//! 2. On `Incoming(JoinerRequest)` — match the carried invitation code
//!    against the parked aggregate (`holder.take_matching`).
//!    * Match → notify the rendezvous service the code is consumed
//!      (best-effort) and record the live session; subsequent handshake
//!      messages (keyslot offer / challenge / confirm) are the concern of
//!      the next phase (P7f).
//!    * Mismatch / expired / malformed first-message → push a
//!      `PairingReject` on the session and close it so the joiner surfaces
//!      a clear error instead of hanging.
//! 3. Follow-up messages (`MessageReceived`) and closures are logged
//!    today and will drive the handshake state machine in P7f.
//!
//! Per `uc-application/AGENTS.md` §11.4 the orchestrator is `pub(crate)`
//! only: external callers reach it indirectly through the owning facade
//! (`SpaceSetupFacade` spawns it during construction).

pub(crate) mod orchestrator;
pub(crate) mod sponsor_handshake;
