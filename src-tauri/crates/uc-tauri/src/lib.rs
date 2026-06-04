//! # uc-tauri
//!
//! Tauri adapter layer for UniClipboard.
//!
//! This crate contains Tauri-specific implementations of ports from uc-core,
//! bootstrap logic for application initialization, and Tauri command handlers.

pub mod activity_hud;
pub mod adapters;
pub mod analytics_forward;
pub mod bootstrap;
pub mod commands;
pub mod lightweight;
pub mod quick_panel;
pub mod run;
pub mod specta_builder;
pub mod tray;
pub mod update_scheduler;

pub use run::run;
