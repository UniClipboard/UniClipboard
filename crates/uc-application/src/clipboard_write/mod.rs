//! Clipboard write coordinator — single boundary for all programmatic
//! clipboard writes (restore / inbound sync / file copy).
//!
//! Moved from `uc-app::usecases::clipboard::clipboard_write_coordinator` in
//! Slice 2 Phase 3 (T0b) as part of the gradual `uc-app` → `uc-application`
//! migration. The old path keeps a deprecated re-export shim until Slice 5 /
//! `uc-app` retirement.
//!
//! Per `uc-application/AGENTS.md` §11.4 this is technically a
//! `Coordinator` (not a `UseCase` / `Facade`), but it is one of the
//! "明确 Coordinator" exceptions named in §18 because it genuinely
//! coordinates a single write boundary across multiple downstream ports
//! (`SystemClipboardPort` + `SelfWriteLedgerPort`) and the guard
//! lifecycle. Public visibility is required by external consumers (daemon
//! workers, tauri runtime).

mod active_register;
mod coordinator;
mod primary_rep_selector;
mod restore_broadcast;
mod timing;

pub use active_register::LocalActiveRegisterAdvancer;
pub use coordinator::{ClipboardWriteCoordinator, ClipboardWriteIntent};
pub use primary_rep_selector::{narrow_to_primary, PrimaryRepError};
pub use restore_broadcast::{RestoreBroadcastRequest, RestoreBroadcastTrigger};
