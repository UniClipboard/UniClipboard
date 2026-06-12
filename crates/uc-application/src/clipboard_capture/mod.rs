//! Clipboard capture use case.
//!
//! Moved from `uc-app::usecases::internal::capture_clipboard` in Slice 2
//! Phase 3 (T0a) as part of the gradual `uc-app` → `uc-application`
//! migration. The old path in `uc-app` remains as a deprecated re-export
//! shim until Slice 5 / `uc-app` retirement, so the 18+ existing consumers
//! continue to compile unchanged.
//!
//! Per `uc-application/AGENTS.md` §11.4 this use case is a valid public
//! export (UseCase types are one of the four permitted public exports from
//! an application-layer module).

mod usecase;

pub use usecase::{CaptureClipboardUseCase, CaptureOutcome};
