//! # uc-tauri
//!
//! Tauri adapter layer for UniClipboard.
//!
//! This crate contains Tauri-specific implementations of ports from uc-core,
//! bootstrap logic for application initialization, and Tauri command handlers.

pub mod adapters;
pub mod bootstrap;
pub mod commands;
pub mod events;
pub mod models;
pub mod protocol;
pub mod services;

#[cfg(test)]
pub mod test_utils;
