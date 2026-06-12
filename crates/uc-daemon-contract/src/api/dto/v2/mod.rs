//! v2 daemon HTTP DTOs (Slice4 Phase 3 T3.2 onward).
//!
//! New stateless contracts living under `/v2/*` HTTP routes. Each
//! domain gets its own submodule (`v2::setup`, `v2::clipboard`, …) so
//! cutting v1 over time is a localised operation.
//!
//! Convention: types here drop the `V2` suffix because the module
//! path (`api::dto::v2::setup::InitializeSpaceRequest`) already
//! signals the version. Call sites disambiguate via `use` rather than
//! by name.

pub mod setup;
