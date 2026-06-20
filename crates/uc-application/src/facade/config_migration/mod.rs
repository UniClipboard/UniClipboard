//! `ConfigMigrationFacade` — whole-installation configuration migration entry
//! point.
//!
//! Aggregates the export / preview / stage-import intents and enforces their
//! business preconditions (initialized / unlocked / uninitialized) before
//! delegating to the migration ports. Per `uc-application/AGENTS.md` §11.4
//! external consumers reach the migration intents only through this facade.

mod facade;

pub use facade::{ConfigMigrationDeps, ConfigMigrationFacade};
