//! Portable ("green") build detection and data-root resolution.
//!
//! A portable build keeps every piece of its data next to the executable
//! instead of the per-user system data directory, so it can run from a USB
//! stick and leave no traces on the host machine. This is the platform-layer
//! entry point for "am I portable, and where does my data live": all
//! upper layers keep calling [`crate::app_dirs`] / the secure-storage factory
//! and transparently follow the redirect.
//!
//! Portable mode is enabled when either:
//!   - the `UC_PORTABLE` environment variable is set to a truthy value
//!     (`1` / `true` / `yes`, case-insensitive) — handy for exercising the
//!     behavior from `cargo run` without shipping a marker file, or
//!   - a marker file named [`PORTABLE_MARKER`] sits next to the executable.
//!
//! The portable zip artifact ships that marker file while the NSIS installer
//! does not, so a single compiled binary serves both distribution forms with
//! no separate build.
//!
//! ## Delegation (ADR-008 P5-0c)
//!
//! The actual detection — the env parse, the marker check, the
//! `OnceLock`-cached resolution — lives in the
//! [`uc-app-paths`](../../uc_app_paths/index.html) directory-layout authority so
//! `uc-platform` and the thin `uc-daemon-process` crate share **one**
//! implementation and **one** cache (no drift). This module re-exports the
//! public API so existing call paths keep resolving:
//!
//!   - external: `uc_platform::portable::is_portable()` (uc-tauri updater), and
//!   - internal: `crate::portable::{portable_data_root, is_portable}`
//!     (app_dirs, secure_storage).

pub use uc_app_paths::{is_portable, portable_data_root, PORTABLE_MARKER};
