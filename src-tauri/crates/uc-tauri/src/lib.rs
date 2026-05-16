//! # uc-tauri
//!
//! Tauri adapter layer for UniClipboard.
//!
//! This crate contains Tauri-specific implementations of ports from uc-core,
//! bootstrap logic for application initialization, and Tauri command handlers.

pub mod adapters;
pub mod bootstrap;
pub mod commands;
pub mod host_event_emitter;
pub mod quick_panel;
pub mod run;
pub mod specta_builder;
pub mod tray;

pub use run::run;
