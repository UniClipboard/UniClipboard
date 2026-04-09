//! # uc-daemon — Headless Daemon Library
//!
//! Provides the [`DaemonService`] trait, placeholder workers,
//! and [`RuntimeState`] for the UniClipboard headless daemon.
//!
//! This crate is used as a library and as a binary (`uniclipboard-daemon`).

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
pub use uc_daemon_contract::DAEMON_API_REVISION;

pub mod api;
pub mod app;
pub mod entrypoint;
pub mod pairing;
pub mod peers;
pub mod process_metadata;
pub mod security;
pub mod service;
pub mod socket;
pub mod state;
pub mod workers;
