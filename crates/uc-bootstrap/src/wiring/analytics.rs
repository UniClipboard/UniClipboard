//! Desktop host wiring for product analytics.

use std::path::PathBuf;
use std::sync::Arc;

use uc_engine::{Engine, Operation, OperationResult};
use uc_observability::analytics::{
    build_event_context, global_event_context, hash_space_id_for_telemetry, load_or_create_ids,
    load_space_person_id, set_global_event_context, AnalyticsIdentityPort, AnalyticsIds,
    AnalyticsPersonId, AnalyticsPort, AppChannel, Event, EventContextInputs, GatedAnalyticsSink,
    InstallSource, LocalAnalyticsIdentity, NoopAnalyticsSink, PosthogSink, StdoutSink,
};

#[derive(Clone)]
pub struct DesktopHostAnalytics {
    sink: Arc<dyn AnalyticsPort>,
    identity: Arc<dyn AnalyticsIdentityPort>,
    ids: AnalyticsIds,
    person_id: AnalyticsPersonId,
}

impl DesktopHostAnalytics {
    pub(crate) fn new(analytics_dir: PathBuf) -> anyhow::Result<Self> {
        let ids = load_or_create_ids(&analytics_dir).inspect_err(|_| {
            tracing::error!(
                error_kind = "analytics_identity_load_failed",
                error = "analytics identity storage unavailable",
                retryable = false,
                "analytics host preparation failed"
            );
        })?;
        let person_id = match load_space_person_id(&analytics_dir) {
            Ok(Some(id)) => AnalyticsPersonId::SpaceShared(id),
            Ok(None) => AnalyticsPersonId::Solo(ids.anonymous_user_id),
            Err(_) => {
                tracing::warn!(
                    error_kind = "analytics_space_identity_load_failed",
                    retryable = false,
                    fallback = "solo",
                    "analytics space identity unavailable"
                );
                AnalyticsPersonId::Solo(ids.anonymous_user_id)
            }
        };

        Ok(Self {
            sink: build_analytics_sink(),
            identity: Arc::new(LocalAnalyticsIdentity::new(analytics_dir.clone())),
            ids,
            person_id,
        })
    }

    pub fn sink(&self) -> Arc<dyn AnalyticsPort> {
        Arc::clone(&self.sink)
    }

    pub(crate) fn identity(&self) -> Arc<dyn AnalyticsIdentityPort> {
        Arc::clone(&self.identity)
    }
}

/// Install the process-level event context before the daemon emits analytics.
#[tracing::instrument(
    name = "analytics.initialize_context",
    level = "info",
    skip_all,
    fields(suppress_device_presence_events = suppress_device_presence_events)
)]
pub async fn initialize_analytics_context(
    analytics: &DesktopHostAnalytics,
    engine: &Engine,
    suppress_device_presence_events: bool,
) {
    if global_event_context().is_some() {
        tracing::debug!(
            result = "already_initialized",
            "analytics context unchanged"
        );
        return;
    }

    let active_device_count = read_active_device_count(engine).await;
    let space_id_hash = read_space_id_hash(engine).await;
    let context = build_event_context(EventContextInputs {
        anonymous_user_id: analytics.ids.anonymous_user_id,
        analytics_device_id: analytics.ids.analytics_device_id,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        app_channel: parse_app_channel(env!("CARGO_PKG_VERSION")),
        install_source: InstallSource::Unknown,
        is_first_run: analytics.ids.is_first_run,
        active_device_count,
        space_id_hash: space_id_hash.clone(),
        analytics_person_id: analytics.person_id.clone(),
    });
    set_global_event_context(Arc::new(context));
    emit_process_open_events(
        analytics.sink.as_ref(),
        analytics.ids.is_first_run,
        suppress_device_presence_events,
    );

    tracing::info!(
        result = "initialized",
        active_device_count,
        has_space = space_id_hash.is_some(),
        is_first_run = analytics.ids.is_first_run,
        "analytics context initialized"
    );
}

async fn read_active_device_count(engine: &Engine) -> u32 {
    match engine.execute(Operation::ListDevices).await {
        Ok(OperationResult::Devices(devices)) => u32::try_from(devices.len()).unwrap_or(u32::MAX),
        Ok(_) => {
            tracing::warn!(
                error_kind = "unexpected_engine_result",
                operation = "list_devices",
                fallback = 0,
                "analytics device count unavailable"
            );
            0
        }
        Err(error) => {
            tracing::warn!(
                error_code = error.code(),
                error_kind = %error.category(),
                retryable = error.is_retryable(),
                operation = "list_devices",
                fallback = 0,
                "analytics device count unavailable"
            );
            0
        }
    }
}

async fn read_space_id_hash(engine: &Engine) -> Option<String> {
    match engine.execute(Operation::QuerySetupState).await {
        Ok(OperationResult::SetupState(state)) => {
            state.space_id.as_deref().map(hash_space_id_for_telemetry)
        }
        Ok(_) => {
            tracing::warn!(
                error_kind = "unexpected_engine_result",
                operation = "query_setup_state",
                fallback = "no_space",
                "analytics space context unavailable"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                error_code = error.code(),
                error_kind = %error.category(),
                retryable = error.is_retryable(),
                operation = "query_setup_state",
                fallback = "no_space",
                "analytics space context unavailable"
            );
            None
        }
    }
}

fn build_analytics_sink() -> Arc<dyn AnalyticsPort> {
    let inner: Arc<dyn AnalyticsPort> = if cfg!(debug_assertions) {
        Arc::new(StdoutSink::new())
    } else {
        let runtime_key = std::env::var("POSTHOG_PROJECT_KEY")
            .ok()
            .filter(|key| !key.is_empty());
        match resolve_posthog_key(runtime_key, option_env!("POSTHOG_PROJECT_KEY")) {
            Some(key) => Arc::new(PosthogSink::new(key)),
            None => {
                tracing::info!(
                    analytics_backend = "noop",
                    reason = "posthog_project_key_missing",
                    "product analytics disabled"
                );
                Arc::new(NoopAnalyticsSink)
            }
        }
    };
    Arc::new(GatedAnalyticsSink::new(inner))
}

fn resolve_posthog_key(runtime: Option<String>, compile: Option<&'static str>) -> Option<String> {
    runtime
        .filter(|key| !key.is_empty())
        .or_else(|| compile.filter(|key| !key.is_empty()).map(String::from))
}

fn parse_app_channel(version: &str) -> AppChannel {
    let Some(suffix) = version.split_once('-').map(|(_, suffix)| suffix) else {
        return AppChannel::Stable;
    };
    match suffix.split(['.', '+']).next().unwrap_or("") {
        "beta" => AppChannel::Beta,
        _ => AppChannel::Alpha,
    }
}

fn emit_process_open_events(
    analytics: &dyn AnalyticsPort,
    is_first_run: bool,
    suppress_device_presence_events: bool,
) {
    if suppress_device_presence_events {
        tracing::debug!(result = "suppressed", "analytics presence events skipped");
        return;
    }
    if is_first_run {
        analytics.capture(Event::AppFirstOpen);
    }
    analytics.capture(Event::AppOpened);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<Event>>,
    }

    impl AnalyticsPort for RecordingSink {
        fn capture(&self, event: Event) {
            self.events
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(event);
        }
    }

    #[test]
    fn runtime_posthog_key_takes_precedence() {
        assert_eq!(
            resolve_posthog_key(Some("runtime".into()), Some("compile")).as_deref(),
            Some("runtime")
        );
    }

    #[test]
    fn empty_posthog_keys_are_ignored() {
        assert_eq!(resolve_posthog_key(Some(String::new()), Some("")), None);
        assert_eq!(
            resolve_posthog_key(Some(String::new()), Some("compile")).as_deref(),
            Some("compile")
        );
    }

    #[test]
    fn app_channel_uses_stable_only_for_clean_versions() {
        assert_eq!(parse_app_channel("1.0.0"), AppChannel::Stable);
        assert_eq!(parse_app_channel("1.0.0-alpha.2"), AppChannel::Alpha);
        assert_eq!(parse_app_channel("1.0.0-beta.1"), AppChannel::Beta);
        assert_eq!(parse_app_channel("1.0.0-rc.1"), AppChannel::Alpha);
    }

    #[test]
    fn process_open_events_respect_first_run_and_suppression() {
        let first_run = RecordingSink::default();
        emit_process_open_events(&first_run, true, false);
        assert_eq!(
            *first_run
                .events
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            vec![Event::AppFirstOpen, Event::AppOpened]
        );

        let returning = RecordingSink::default();
        emit_process_open_events(&returning, false, false);
        assert_eq!(
            *returning
                .events
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            vec![Event::AppOpened]
        );

        let suppressed = RecordingSink::default();
        emit_process_open_events(&suppressed, true, true);
        assert!(suppressed
            .events
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_empty());
    }
}
