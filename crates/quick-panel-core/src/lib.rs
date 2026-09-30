//! Platform-independent logic of the GPUI quick panel.
//!
//! Nothing here may depend on GPUI, AppKit or an async runtime; the app crate supplies those and
//! implements the ports declared here.

pub mod actions;
pub mod content;
pub mod empty_page;
pub mod geometry;
pub mod grid;
pub mod language;
pub mod ports;
pub mod query;
pub mod selection;
pub mod state;
pub mod text;
