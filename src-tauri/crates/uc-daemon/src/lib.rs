//! # uc-daemon — Headless Daemon Library
//!
//! Provides the [`DaemonService`] trait, placeholder workers, shared RPC types,
//! and [`RuntimeState`] for the UniClipboard headless daemon.
//!
//! This crate is used as both a library (by uc-cli for RPC type sharing) and
//! a binary (`uniclipboard-daemon`).

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DAEMON_API_REVISION: &str = "setup-pairing-http-routes-v1";

pub mod api;
pub mod app;
pub mod entrypoint;
pub mod pairing;
pub mod peers;
pub mod process_metadata;
pub mod rpc;
pub mod service;
pub mod socket;
pub mod security;
pub mod state;
pub mod workers;
