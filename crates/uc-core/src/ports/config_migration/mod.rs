//! Whole-installation configuration migration ports and domain model.
//!
//! This module abstracts moving a complete installation's configuration —
//! including its device identity, encrypted data, and settings — from one
//! machine or storage layout to another, as a single password-protected
//! bundle. The domain treats a bundle as an opaque, secret-bearing artifact
//! referenced by a filesystem location; packaging, encryption, key derivation,
//! key naming, and persistence are infrastructure concerns and never appear in
//! these signatures.

pub mod error;
pub mod model;
pub mod ports;

pub use error::ConfigMigrationError;
pub use model::{ConfigImportPreview, ConfigSourceMode, StagedConfigImport};
pub use ports::{ExportConfigBundlePort, PreviewConfigImportPort, StageConfigImportPort};
