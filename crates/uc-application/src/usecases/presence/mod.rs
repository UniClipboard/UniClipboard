//! Presence-related application use cases (Slice 2 Phase 1).
//!
//! Consumed by `MemberRosterFacade` (T7) and `SpaceSetupFacade::auto_start_network`
//! (T8 — F1 hook) to pre-connect every paired device after unlock / resume.

pub(crate) mod ensure_reachable_all;
