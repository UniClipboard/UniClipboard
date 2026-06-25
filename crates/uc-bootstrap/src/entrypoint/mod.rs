//! Scenario entry constructors.
//!
//! Each module builds the dependency graph for one runtime scenario: the
//! daemon lifecycle, the CLI dev-tools in-process facade, and the headless
//! (non-GUI) runtime shared by both.

pub mod cli;
pub mod daemon;
pub mod non_gui;
