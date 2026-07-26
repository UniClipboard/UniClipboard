//! Host implementation of the portable analytics identity contract.

use std::path::PathBuf;
use std::sync::Arc;

use uc_observability_contract::analytics::{
    AdoptOutcome, AnalyticsIdentityError, AnalyticsIdentityPort, ReleaseOutcome,
};
use uuid::Uuid;

use super::context::{global_event_context, set_global_event_context, AnalyticsPersonId};
use super::ids::{clear_space_person_id, set_space_person_id};

pub use uc_observability_contract::analytics::{
    hash_space_id_for_telemetry, NoopAnalyticsIdentity,
};

/// File-backed analytics identity adapter owned by the host process.
pub struct LocalAnalyticsIdentity {
    analytics_dir: PathBuf,
}

impl LocalAnalyticsIdentity {
    pub fn new(analytics_dir: PathBuf) -> Self {
        Self { analytics_dir }
    }
}

impl AnalyticsIdentityPort for LocalAnalyticsIdentity {
    fn adopt_space_person(
        &self,
        space_person_id: Uuid,
    ) -> Result<AdoptOutcome, AnalyticsIdentityError> {
        let current =
            global_event_context().ok_or(AnalyticsIdentityError::ContextNotInitialised)?;
        let previous = current.analytics_person_id.as_uuid();

        set_space_person_id(&self.analytics_dir, space_person_id)
            .map_err(AnalyticsIdentityError::PersistFailed)?;

        let mut new_context = (*current).clone();
        new_context.analytics_person_id = AnalyticsPersonId::SpaceShared(space_person_id);
        set_global_event_context(Arc::new(new_context));

        Ok(AdoptOutcome {
            previous_distinct_id: previous,
            new_distinct_id: space_person_id,
        })
    }

    fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        let current =
            global_event_context().ok_or(AnalyticsIdentityError::ContextNotInitialised)?;
        let previous = current.analytics_person_id.as_uuid();
        let new_identity = current.anonymous_user_id;

        clear_space_person_id(&self.analytics_dir)
            .map_err(AnalyticsIdentityError::PersistFailed)?;

        let mut new_context = (*current).clone();
        new_context.analytics_person_id = AnalyticsPersonId::Solo(new_identity);
        set_global_event_context(Arc::new(new_context));

        Ok(ReleaseOutcome {
            previous_distinct_id: previous,
            new_distinct_id: new_identity,
        })
    }

    fn current_space_person_id(&self) -> Option<Uuid> {
        match super::ids::load_space_person_id(&self.analytics_dir) {
            Ok(identity) => identity,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "failed to load the analytics space identity; using solo identity"
                );
                None
            }
        }
    }

    fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        let current =
            global_event_context().ok_or(AnalyticsIdentityError::ContextNotInitialised)?;
        let previous = current.analytics_person_id.as_uuid();

        super::ids::clear_space_person_id(&self.analytics_dir)
            .map_err(AnalyticsIdentityError::PersistFailed)?;
        super::ids::reset(&self.analytics_dir).map_err(AnalyticsIdentityError::PersistFailed)?;
        let new_ids = super::ids::load_or_create(&self.analytics_dir)
            .map_err(AnalyticsIdentityError::PersistFailed)?;

        let mut new_context = (*current).clone();
        new_context.anonymous_user_id = new_ids.anonymous_user_id;
        new_context.analytics_device_id = new_ids.analytics_device_id;
        new_context.analytics_person_id = AnalyticsPersonId::Solo(new_ids.anonymous_user_id);
        set_global_event_context(Arc::new(new_context));

        Ok(ReleaseOutcome {
            previous_distinct_id: previous,
            new_distinct_id: new_ids.anonymous_user_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::{
        build_event_context, clear_global_event_context, lock_global_event_context_for_tests,
        AnalyticsPersonId, AppChannel, EventContextInputs, InstallSource,
    };
    use super::*;
    use tempfile::TempDir;

    fn install_solo_context(anonymous_id: Uuid) {
        let context = build_event_context(EventContextInputs {
            anonymous_user_id: anonymous_id,
            analytics_device_id: Uuid::now_v7(),
            app_version: "test".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Unknown,
            is_first_run: false,
            active_device_count: 1,
            space_id_hash: None,
            analytics_person_id: AnalyticsPersonId::Solo(anonymous_id),
        });
        set_global_event_context(Arc::new(context));
    }

    #[test]
    fn adopt_and_release_update_persistence_and_context() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();
        let directory = TempDir::new().expect("temp analytics directory");
        let adapter = LocalAnalyticsIdentity::new(directory.path().to_path_buf());
        let anonymous_id = Uuid::now_v7();
        install_solo_context(anonymous_id);

        let space_person_id = Uuid::now_v7();
        let adopted = adapter
            .adopt_space_person(space_person_id)
            .expect("adopt analytics identity");
        assert_eq!(adopted.previous_distinct_id, anonymous_id);
        assert_eq!(adopted.new_distinct_id, space_person_id);
        assert_eq!(
            super::super::ids::load_space_person_id(directory.path())
                .expect("load persisted identity"),
            Some(space_person_id)
        );

        let released = adapter
            .release_space_person()
            .expect("release analytics identity");
        assert_eq!(released.new_distinct_id, anonymous_id);
        assert!(super::super::ids::load_space_person_id(directory.path())
            .expect("load released identity")
            .is_none());
        clear_global_event_context();
    }

    #[test]
    fn identity_change_requires_an_installed_context() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();
        let directory = TempDir::new().expect("temp analytics directory");
        let adapter = LocalAnalyticsIdentity::new(directory.path().to_path_buf());

        assert!(matches!(
            adapter.adopt_space_person(Uuid::now_v7()),
            Err(AnalyticsIdentityError::ContextNotInitialised)
        ));
    }
}
