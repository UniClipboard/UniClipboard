//! Stateless v2 daemon HTTP surface (Slice4 Phase 3 T3.2 onward).
//!
//! Houses every artefact for the `/v2/*` HTTP routes — handlers,
//! per-domain `pub fn router()` factories, and the aggregator
//! [`router`] used by [`crate::api::routes`] to mount the whole v2
//! tree in one call. Future v2 domains land here as siblings of
//! `setup.rs` so T3.4's deletion of the legacy surface stays
//! mechanical.

use axum::Router;

use crate::api::server::DaemonApiState;

pub mod setup;

/// Build the aggregated v2 router. Call sites in
/// [`crate::api::routes`] mount this with a single `.merge(...)`.
pub fn router() -> Router<DaemonApiState> {
    Router::new().merge(setup::router())
}
