//! Search transport DTOs — re-exported from the shared daemon-contract crate.
//!
//! The single source of truth for search response envelopes lives in
//! `uc-daemon-contract`. This module is a thin pass-through so all existing
//! daemon code at `crate::api::dto::search::*` continues to resolve.
pub use uc_daemon_contract::api::dto::search::*;
