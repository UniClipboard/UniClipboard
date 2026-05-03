//! # uc-tauri
//!
//! Tauri adapter layer for UniClipboard.
//!
//! This crate contains Tauri-specific implementations of ports from uc-core,
//! bootstrap logic for application initialization, and Tauri command handlers.

pub mod adapters;
pub mod bootstrap;
pub mod commands;
pub mod plugins;
pub mod quick_panel;
pub mod run;
pub mod tray;

pub use run::run;
