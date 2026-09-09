//! # uc-tauri
//!
//! Tauri adapter layer for UniClipboard.
//!
//! This crate contains the Tauri shell, application bootstrap, and command handlers.

#[cfg(all(feature = "e2e", not(debug_assertions)))]
compile_error!("the e2e feature is restricted to debug builds");

pub mod activity_hud;
pub mod adapters;
pub mod analytics_forward;
pub mod bootstrap;
pub mod commands;
mod desktop_theme;
pub mod lightweight;
pub mod main_window;
pub mod modifier_double_tap_platform;
pub mod process_environment;
pub mod quick_panel;
pub mod run;
mod runtime_environment;
pub mod specta_builder;
pub mod tray;
pub mod update_scheduler;
pub mod visual_effects;
mod visual_effects_probe;
mod visual_effects_storage;
mod window_frame_environment;

pub use process_environment::prepare_process_environment;
pub use run::run;
