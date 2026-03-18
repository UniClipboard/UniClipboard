//! # uc-daemon — Headless Daemon Library
//!
//! Provides the [`DaemonWorker`] trait, placeholder workers, shared RPC types,
//! and [`RuntimeState`] for the UniClipboard headless daemon.
//!
//! This crate is used as both a library (by uc-cli for RPC type sharing) and
//! a binary (`uniclipboard-daemon`).

pub mod app;
pub mod rpc;
pub mod socket;
pub mod state;
pub mod worker;
pub mod workers;
