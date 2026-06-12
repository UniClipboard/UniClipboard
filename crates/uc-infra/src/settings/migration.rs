use uc_core::ports::SettingsMigrationPort;
use uc_core::settings::model::{Settings, CURRENT_SCHEMA_VERSION};

/// Error type for settings migration failures.
#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    /// No migration found for the given schema version.
    #[error("no migration found from schema version {from_version}")]
    NoMigrationFound { from_version: u32 },

    /// Migration did not increment the schema_version.
    #[error("migration from version {from_version} did not increment schema_version")]
    VersionNotIncremented { from_version: u32 },

    /// Migration loop exceeded maximum iterations (possible infinite loop).
    #[error("migration loop exceeded {iterations} iterations, possible infinite loop")]
    MaxIterationsExceeded { iterations: u32 },
}

pub struct SettingsMigrator {
    migrations: Vec<Box<dyn SettingsMigrationPort>>,
}

impl SettingsMigrator {
    /// Creates a new `SettingsMigrator` with its migrations list initialized.
    ///
    /// The migrations vector is currently empty with placeholder comments where future
    /// migrations can be added.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_infra::settings::migration::SettingsMigrator;
    /// let migrator = SettingsMigrator::new();
    /// ```
    pub fn new() -> Self {
        Self {
            migrations: vec![
                // Box::new(MigrationV1ToV2),
                // Box::new(MigrationV2ToV3),
            ],
        }
    }

    /// Migrates a Settings instance forward until its schema_version equals CURRENT_SCHEMA_VERSION.
    ///
    /// Applies successive migrations from the migrator's migration list starting at the settings'
    /// current `schema_version`. Returns an error if no migration is available for the next required version.
    ///
    /// # Returns
    ///
    /// `Ok(Settings)` updated to `CURRENT_SCHEMA_VERSION`, or `Err(MigrationError)` if migration fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use uc_infra::settings::migration::SettingsMigrator;
    /// # use uc_core::settings::model::Settings;
    /// # use uc_infra::settings::migration::MigrationError;
    /// let migrator = SettingsMigrator::new();
    /// let settings = Settings::default();
    /// let migrated = migrator.migrate_to_latest(settings)?;
    /// # Ok::<(), MigrationError>(())
    /// ```
    pub fn migrate_to_latest(&self, mut settings: Settings) -> Result<Settings, MigrationError> {
        let mut iterations = 0;
        const MAX_ITERATIONS: u32 = 100;

        loop {
            let current = settings.schema_version;

            if current >= CURRENT_SCHEMA_VERSION {
                break;
            }

            iterations += 1;
            if iterations > MAX_ITERATIONS {
                return Err(MigrationError::MaxIterationsExceeded {
                    iterations: MAX_ITERATIONS,
                });
            }

            let migration = self
                .migrations
                .iter()
                .find(|m| m.from_version() == current)
                .ok_or_else(|| MigrationError::NoMigrationFound {
                    from_version: current,
                })?;

            let new_settings = migration.migrate(settings);
            if new_settings.schema_version <= current {
                return Err(MigrationError::VersionNotIncremented {
                    from_version: current,
                });
            }
            settings = new_settings;
        }

        Ok(settings)
    }
}
