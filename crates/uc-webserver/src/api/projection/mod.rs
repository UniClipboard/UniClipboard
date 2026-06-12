//! Owned projection layer between application-facade views and wire DTOs.
//!
//! This module is the single home for the webserver's cross-crate type
//! conversions (architecture-rules §Cross-Crate Type Conversion): both the
//! facade view types (`uc-application`) and the wire DTOs
//! (`uc-daemon-contract`) are foreign to this crate, so `From`/`Into` impls
//! are impossible and the projection rules live here behind local traits.
//!
//! - [`IntoApiDto`] — pure projection from a facade view onto its wire DTO.
//! - [`IntoDomain`] — pure projection from a request DTO onto the facade
//!   input shape (patches, commands).
//!
//! Context-dependent projections (ones that need data beyond `self`, e.g. the
//! server version) stay as named mapper functions in the same submodule.
//!
//! Handler files must not define ad-hoc `*_to_dto` / `*_from_dto` helpers;
//! add the impl to the matching submodule instead so each DTO's field mapping
//! has exactly one source of truth.

pub mod clipboard;
pub mod member;
pub mod search;
pub mod settings;
pub mod setup_v2;
pub mod upgrade;

/// Pure projection from an application-facade view onto its wire DTO.
pub trait IntoApiDto<T> {
    fn into_api_dto(self) -> T;
}

/// Pure projection from a request DTO onto the application-facade input shape.
pub trait IntoDomain<T> {
    fn into_domain(self) -> T;
}
