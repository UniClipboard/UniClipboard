use std::time::Duration;

use tracing::info;
use uc_core::ports::SettingsMigrationPort;
use uc_core::settings::model::{
    RetentionPolicy, RetentionRule, RuleEvaluation, Settings, StartupMode, CURRENT_SCHEMA_VERSION,
};

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

/// v1 -> v2: one-time rewrite of a stock v1-default `retention_policy`.
///
/// `retention_policy` was persisted since early settings versions but never
/// enforced by any deletion path until v2 shipped real enforcement
/// (`EnforceRetentionPolicyUseCase`). An install that still carries the exact
/// v1 stock default (`ByAge(30d) + ByCount(500)`) never made a deliberate
/// choice about it — that value is the byproduct of a dead config field, not
/// a user decision. Enforcing it as-is the moment it goes live would mass-
/// delete history older than 30 days / past the 500th item on the very first
/// cleanup tick, with no warning.
///
/// This migration rewrites that exact stock value to the new default
/// (`ByAge(180d)`, no count cap) — the same softened default a brand-new
/// install gets. Any `retention_policy` that differs from the v1 stock
/// default (the user changed a rule, disabled it, or picked different
/// values) reflects a deliberate choice and is left untouched.
struct MigrationV1ToV2;

impl SettingsMigrationPort for MigrationV1ToV2 {
    fn from_version(&self) -> u32 {
        1
    }

    fn to_version(&self) -> u32 {
        2
    }

    fn migrate(&self, mut settings: Settings) -> Settings {
        if is_v1_stock_default_retention_policy(&settings.retention_policy) {
            info!("Rewriting stale v1 stock retention_policy default (30d/500 items) to v2 default (180d/unlimited)");
            settings.retention_policy = RetentionPolicy::default();
        }
        settings.schema_version = self.to_version();
        settings
    }
}

/// Whether `policy` is exactly the v1 stock default (`ByAge(30d) +
/// ByCount(500)`, enabled, skip_pinned, `AnyMatch`) — never customized by the
/// user. Compares against that historical shape literally (not against
/// [`RetentionPolicy::default()`], which now returns the v2 default) since
/// this check exists specifically to detect the pre-migration stock value.
fn is_v1_stock_default_retention_policy(policy: &RetentionPolicy) -> bool {
    const V1_DEFAULT_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 30);
    const V1_DEFAULT_MAX_ITEMS: usize = 500;

    policy.enabled
        && policy.skip_pinned
        && matches!(policy.evaluation, RuleEvaluation::AnyMatch)
        && policy.rules.len() == 2
        && matches!(
            policy.rules.first(),
            Some(RetentionRule::ByAge { max_age }) if *max_age == V1_DEFAULT_MAX_AGE
        )
        && matches!(
            policy.rules.get(1),
            Some(RetentionRule::ByCount { max_items }) if *max_items == V1_DEFAULT_MAX_ITEMS
        )
}

/// v2 -> v3: one-time rewrite of the legacy `silent_start` /
/// `lightweight_start` booleans into the mutually-exclusive `startup_mode`
/// enum (issue #1169 follow-up: the two booleans were never truly
/// independent, and the UI hard-coupled `lightweight_start` to requiring
/// `auto_start` to be on).
///
/// If both legacy booleans are set (never produced by this app's own UI,
/// but a defensive case for hand-edited settings.json), `Lightweight` wins
/// as the "stronger" background mode.
struct MigrationV2ToV3;

impl SettingsMigrationPort for MigrationV2ToV3 {
    fn from_version(&self) -> u32 {
        2
    }

    fn to_version(&self) -> u32 {
        3
    }

    fn migrate(&self, mut settings: Settings) -> Settings {
        settings.general.startup_mode = if settings.general.lightweight_start {
            StartupMode::Lightweight
        } else if settings.general.silent_start {
            StartupMode::Silent
        } else {
            StartupMode::Normal
        };
        settings.schema_version = self.to_version();
        settings
    }
}

pub struct SettingsMigrator {
    migrations: Vec<Box<dyn SettingsMigrationPort>>,
}

impl SettingsMigrator {
    /// Creates a new `SettingsMigrator` with its migrations list initialized.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_infra::settings::migration::SettingsMigrator;
    /// let migrator = SettingsMigrator::new();
    /// ```
    pub fn new() -> Self {
        Self {
            migrations: vec![Box::new(MigrationV1ToV2), Box::new(MigrationV2ToV3)],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn v1_stock_retention_policy() -> RetentionPolicy {
        RetentionPolicy {
            enabled: true,
            skip_pinned: true,
            evaluation: RuleEvaluation::AnyMatch,
            rules: vec![
                RetentionRule::ByAge {
                    max_age: Duration::from_secs(60 * 60 * 24 * 30),
                },
                RetentionRule::ByCount { max_items: 500 },
            ],
        }
    }

    #[test]
    fn stock_v1_default_retention_policy_migrates_to_v2_default() {
        let settings = Settings {
            schema_version: 1,
            retention_policy: v1_stock_retention_policy(),
            ..Settings::default()
        };

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.retention_policy.rules.len(), 1);
        assert!(matches!(
            migrated.retention_policy.rules.first(),
            Some(RetentionRule::ByAge { max_age }) if *max_age == Duration::from_secs(60 * 60 * 24 * 180)
        ));
    }

    #[test]
    fn versionless_settings_with_stock_retention_policy_still_migrates() {
        // A settings.json missing `schema_version` entirely (never produced
        // by this app itself, but a defensive case for hand-edited or
        // externally generated files) must not be mistaken for
        // "already current" and silently skip this migration.
        let json = r#"{
            "retention_policy": {
                "enabled": true,
                "skip_pinned": true,
                "evaluation": "any_match",
                "rules": [
                    { "by_age": { "max_age": 2592000 } },
                    { "by_count": { "max_items": 500 } }
                ]
            }
        }"#;
        let settings: Settings = serde_json::from_str(json).expect("parse versionless settings");
        assert_eq!(settings.schema_version, 1);

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.retention_policy.rules.len(), 1);
        assert!(matches!(
            migrated.retention_policy.rules.first(),
            Some(RetentionRule::ByAge { max_age }) if *max_age == Duration::from_secs(60 * 60 * 24 * 180)
        ));
    }

    #[test]
    fn customized_retention_policy_survives_migration_untouched() {
        let mut customized = v1_stock_retention_policy();
        customized.skip_pinned = false; // user explicitly turned this off
        let settings = Settings {
            schema_version: 1,
            retention_policy: customized.clone(),
            ..Settings::default()
        };

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.retention_policy.rules, customized.rules);
        assert!(!migrated.retention_policy.skip_pinned);
    }

    #[test]
    fn disabled_stock_retention_policy_is_not_re_enabled() {
        let mut disabled = v1_stock_retention_policy();
        disabled.enabled = false; // user explicitly disabled retention entirely
        let settings = Settings {
            schema_version: 1,
            retention_policy: disabled,
            ..Settings::default()
        };

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!migrated.retention_policy.enabled);
        assert_eq!(migrated.retention_policy.rules.len(), 2); // untouched stock rules
    }

    #[test]
    fn already_current_schema_version_is_a_no_op() {
        let settings = Settings::default();
        assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);

        let migrated = SettingsMigrator::new()
            .migrate_to_latest(settings.clone())
            .unwrap();

        assert_eq!(
            migrated.retention_policy.rules,
            settings.retention_policy.rules
        );
    }

    #[test]
    fn v2_lightweight_start_migrates_to_startup_mode_lightweight() {
        let mut settings = Settings {
            schema_version: 2,
            ..Settings::default()
        };
        settings.general.lightweight_start = true;

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.general.startup_mode, StartupMode::Lightweight);
    }

    #[test]
    fn v2_silent_start_alone_migrates_to_startup_mode_silent() {
        let mut settings = Settings {
            schema_version: 2,
            ..Settings::default()
        };
        settings.general.silent_start = true;

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.general.startup_mode, StartupMode::Silent);
    }

    #[test]
    fn v2_neither_legacy_flag_migrates_to_startup_mode_normal() {
        let settings = Settings {
            schema_version: 2,
            ..Settings::default()
        };

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.general.startup_mode, StartupMode::Normal);
    }

    #[test]
    fn v2_both_legacy_flags_set_lightweight_wins() {
        let mut settings = Settings {
            schema_version: 2,
            ..Settings::default()
        };
        settings.general.silent_start = true;
        settings.general.lightweight_start = true;

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.general.startup_mode, StartupMode::Lightweight);
    }

    #[test]
    fn versionless_settings_with_lightweight_start_migrates_through_v1_and_v2() {
        // A settings.json missing `schema_version` entirely reads as v1
        // (see `versionless_settings_with_stock_retention_policy_still_migrates`
        // above); this exercises the full v1 -> v2 -> v3 chain in one call.
        let json = r#"{
            "general": { "lightweight_start": true }
        }"#;
        let settings: Settings = serde_json::from_str(json).expect("parse versionless settings");
        assert_eq!(settings.schema_version, 1);

        let migrated = SettingsMigrator::new().migrate_to_latest(settings).unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.general.startup_mode, StartupMode::Lightweight);
    }
}
