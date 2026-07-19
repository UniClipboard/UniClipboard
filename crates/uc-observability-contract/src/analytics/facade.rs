//! Application-facing entry point of the analytics subsystem.
//!
//! Capture, identity persistence, and PostHog `$identify` / `$groupidentify`
//! handshakes are three internal mechanisms — but every caller that touches
//! analytics ends up combining them in the same fixed sequences:
//!
//! - sponsor mints a new Space at A1 setup: adopt → `$identify` → `$groupidentify(initial)`
//! - joiner accepts a sponsor-issued id at A2 / switch_space: adopt → `$identify`
//! - target Space has no person yet at switch_space: release → `$identify`
//! - user resets telemetry: reset → `$identify(new anon)`
//!
//! Exposing the raw [`AnalyticsPort`] + [`AnalyticsIdentityPort`] pair to
//! every call site spreads these rules across five use cases. This facade
//! collapses them into one entry per scenario so the application layer only
//! states *what kind of identity change happened*, not which payload to
//! mint in which order or what to skip on failure.
//!
//! Invariants owned by this module (callers must not depend on them):
//!
//! - adopt-then-identify ordering; identify is never emitted before the
//!   persistent state and global `EventContext` are switched
//! - identify / group_identify are skipped when the underlying
//!   `AnalyticsIdentityPort` adopt or release fails
//! - `$groupidentify` is only emitted when *this* device is creating the
//!   group (the self-minted path); subsequent devices join the group via
//!   the `$groups` field on each captured event

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::events::Event;
use super::identity::{hash_space_id_for_telemetry, AnalyticsIdentityError, AnalyticsIdentityPort};
use super::port::{AnalyticsPort, GroupIdentifyPayload, IdentifyPayload};

/// Application-facing entry point covering both analytics capture and
/// identity transitions.
pub trait AnalyticsFacade: Send + Sync {
    /// Fire-and-forget capture of a product event.
    fn capture(&self, event: Event);

    /// Sponsor finishes creating a new Space. Mints a fresh
    /// `space_person_id`, persists it, rebuilds the global EventContext,
    /// emits `$identify` linking the previous anonymous id to the new
    /// person, and `$groupidentify` to declare the brand-new Space group
    /// with `device_count = 1` and `created_at = now`.
    ///
    /// On adopt failure: warn-log, skip identify / group_identify. The
    /// caller's subsequent captures still fire under the Solo identity.
    fn adopt_self_minted(&self, req: SelfMintedAdoptRequest);

    /// Joiner (or switch_space target) accepts a sponsor-issued
    /// `space_person_id`. Persists it, rebuilds context, emits `$identify`.
    /// No `$groupidentify` — the group was already declared by its creator.
    fn adopt_from_sponsor(&self, space_person_id: Uuid);

    /// switch_space when the target sponsor has no `space_person_id` yet,
    /// or any other path that should fall back to Solo. Releases local
    /// state, rebuilds context, emits `$identify` back to the local
    /// anonymous id.
    fn release_to_solo(&self);

    /// User-initiated reset: clear `space_person_id`, regenerate
    /// `anonymous_user_id` and `analytics_device_id`, rebuild context,
    /// then `$identify` to the fresh anonymous id.
    ///
    /// Unlike the other transition methods this one surfaces errors —
    /// the UI needs to know whether the reset actually went through.
    fn reset_identity(&self) -> Result<(), ResetIdentityError>;

    /// Locally-persisted `space_person_id`, or `None` when this device has
    /// never accepted or minted one. Sponsor handshake reads this to fill
    /// `SponsorConfirm.sponsor_space_person_id`.
    fn current_space_person_id(&self) -> Option<Uuid>;
}

/// Inputs to [`AnalyticsFacade::adopt_self_minted`].
#[derive(Debug, Clone)]
pub struct SelfMintedAdoptRequest {
    /// The newly-created Space id. Hashed irreversibly before leaving the
    /// device; only the 16-hex prefix ever reaches PostHog as the group key.
    pub space_id: String,
    /// `now()` in epoch milliseconds — written into `created_at` on the
    /// freshly-declared group. Passing it in keeps the facade independent
    /// of any clock port.
    pub now_ms: i64,
}

/// Surface-level error for [`AnalyticsFacade::reset_identity`].
#[derive(Debug)]
pub enum ResetIdentityError {
    /// Underlying storage operation failed; identity remains in its
    /// previous state.
    Storage(String),
}

impl std::fmt::Display for ResetIdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(msg) => write!(f, "reset telemetry identity failed: {msg}"),
        }
    }
}

impl std::error::Error for ResetIdentityError {}

/// Default composition of an [`AnalyticsPort`] sink and an
/// [`AnalyticsIdentityPort`]. The sequencing rules live here and only here.
pub struct DefaultAnalyticsFacade {
    sink: Arc<dyn AnalyticsPort>,
    identity: Arc<dyn AnalyticsIdentityPort>,
}

impl DefaultAnalyticsFacade {
    pub fn new(sink: Arc<dyn AnalyticsPort>, identity: Arc<dyn AnalyticsIdentityPort>) -> Self {
        Self { sink, identity }
    }
}

impl AnalyticsFacade for DefaultAnalyticsFacade {
    fn capture(&self, event: Event) {
        self.sink.capture(event);
    }

    fn adopt_self_minted(&self, req: SelfMintedAdoptRequest) {
        let space_person_id = Uuid::now_v7();
        match self.identity.adopt_space_person(space_person_id) {
            Ok(outcome) => {
                self.sink.identify(IdentifyPayload::switch_only(
                    outcome.previous_distinct_id,
                    outcome.new_distinct_id,
                ));
                let group_key = hash_space_id_for_telemetry(&req.space_id);
                let mut set = Map::new();
                set.insert(
                    "created_at".into(),
                    Value::String(
                        DateTime::<Utc>::from_timestamp_millis(req.now_ms)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default(),
                    ),
                );
                set.insert("device_count".into(), Value::Number(1.into()));
                self.sink
                    .group_identify(GroupIdentifyPayload::for_space(group_key, set));
            }
            Err(err) => warn_adopt("adopt_self_minted", &err),
        }
    }

    fn adopt_from_sponsor(&self, space_person_id: Uuid) {
        match self.identity.adopt_space_person(space_person_id) {
            Ok(outcome) => {
                self.sink.identify(IdentifyPayload::switch_only(
                    outcome.previous_distinct_id,
                    outcome.new_distinct_id,
                ));
            }
            Err(err) => warn_adopt("adopt_from_sponsor", &err),
        }
    }

    fn release_to_solo(&self) {
        match self.identity.release_space_person() {
            Ok(outcome) => {
                self.sink.identify(IdentifyPayload::switch_only(
                    outcome.previous_distinct_id,
                    outcome.new_distinct_id,
                ));
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "release_to_solo: identity release failed; identity left in old state"
                );
            }
        }
    }

    fn reset_identity(&self) -> Result<(), ResetIdentityError> {
        let outcome = self
            .identity
            .reset_telemetry_identity()
            .map_err(|e| ResetIdentityError::Storage(e.to_string()))?;
        self.sink.identify(IdentifyPayload::switch_only(
            outcome.previous_distinct_id,
            outcome.new_distinct_id,
        ));
        Ok(())
    }

    fn current_space_person_id(&self) -> Option<Uuid> {
        self.identity.current_space_person_id()
    }
}

fn warn_adopt(scope: &str, err: &AnalyticsIdentityError) {
    tracing::warn!(
        scope,
        error = %err,
        "analytics identity adopt failed; person aggregation deferred"
    );
}

/// Test / disabled fallback. All methods are inert; `reset_identity`
/// returns `Ok(())` so call sites can exercise the success path without
/// staging a real identity port.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAnalyticsFacade;

impl AnalyticsFacade for NoopAnalyticsFacade {
    fn capture(&self, _: Event) {}
    fn adopt_self_minted(&self, _: SelfMintedAdoptRequest) {}
    fn adopt_from_sponsor(&self, _: Uuid) {}
    fn release_to_solo(&self) {}
    fn reset_identity(&self) -> Result<(), ResetIdentityError> {
        Ok(())
    }
    fn current_space_person_id(&self) -> Option<Uuid> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use super::super::identity::{AdoptOutcome, ReleaseOutcome};

    #[derive(Default)]
    struct RecordingSink {
        captures: Mutex<Vec<Event>>,
        identifies: Mutex<Vec<IdentifyPayload>>,
        group_identifies: Mutex<Vec<GroupIdentifyPayload>>,
    }

    impl AnalyticsPort for RecordingSink {
        fn capture(&self, event: Event) {
            self.captures.lock().unwrap().push(event);
        }
        fn identify(&self, payload: IdentifyPayload) {
            self.identifies.lock().unwrap().push(payload);
        }
        fn group_identify(&self, payload: GroupIdentifyPayload) {
            self.group_identifies.lock().unwrap().push(payload);
        }
    }

    #[derive(Default)]
    struct RecordingIdentity {
        adopts: Mutex<Vec<Uuid>>,
        releases: Mutex<u32>,
        resets: Mutex<u32>,
    }

    impl AnalyticsIdentityPort for RecordingIdentity {
        fn adopt_space_person(&self, id: Uuid) -> Result<AdoptOutcome, AnalyticsIdentityError> {
            self.adopts.lock().unwrap().push(id);
            Ok(AdoptOutcome {
                previous_distinct_id: Uuid::nil(),
                new_distinct_id: id,
            })
        }
        fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
            *self.releases.lock().unwrap() += 1;
            Ok(ReleaseOutcome {
                previous_distinct_id: Uuid::nil(),
                new_distinct_id: Uuid::nil(),
            })
        }
        fn current_space_person_id(&self) -> Option<Uuid> {
            None
        }
        fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
            *self.resets.lock().unwrap() += 1;
            Ok(ReleaseOutcome {
                previous_distinct_id: Uuid::nil(),
                new_distinct_id: Uuid::nil(),
            })
        }
    }

    struct AlwaysFailingIdentity;

    impl AnalyticsIdentityPort for AlwaysFailingIdentity {
        fn adopt_space_person(&self, _: Uuid) -> Result<AdoptOutcome, AnalyticsIdentityError> {
            Err(AnalyticsIdentityError::ContextNotInitialised)
        }
        fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
            Err(AnalyticsIdentityError::ContextNotInitialised)
        }
        fn current_space_person_id(&self) -> Option<Uuid> {
            None
        }
        fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
            Err(AnalyticsIdentityError::ContextNotInitialised)
        }
    }

    #[test]
    fn adopt_self_minted_runs_adopt_then_identify_then_group_identify() {
        let sink = Arc::new(RecordingSink::default());
        let identity = Arc::new(RecordingIdentity::default());
        let facade =
            DefaultAnalyticsFacade::new(sink.clone() as Arc<dyn AnalyticsPort>, identity.clone());

        facade.adopt_self_minted(SelfMintedAdoptRequest {
            space_id: "space-xyz".into(),
            now_ms: 1_700_000_000_000,
        });

        assert_eq!(identity.adopts.lock().unwrap().len(), 1);
        assert_eq!(sink.identifies.lock().unwrap().len(), 1);
        let g = sink.group_identifies.lock().unwrap();
        assert_eq!(
            g.len(),
            1,
            "group_identify must fire on the self-minted path"
        );
        assert_eq!(g[0].group_type, "space");
        assert!(g[0].set.contains_key("created_at"));
        assert!(g[0].set.contains_key("device_count"));
    }

    #[test]
    fn adopt_self_minted_skips_identify_when_adopt_fails() {
        let sink = Arc::new(RecordingSink::default());
        let facade = DefaultAnalyticsFacade::new(
            sink.clone() as Arc<dyn AnalyticsPort>,
            Arc::new(AlwaysFailingIdentity),
        );

        facade.adopt_self_minted(SelfMintedAdoptRequest {
            space_id: "space-xyz".into(),
            now_ms: 0,
        });

        assert!(sink.identifies.lock().unwrap().is_empty());
        assert!(sink.group_identifies.lock().unwrap().is_empty());
    }

    #[test]
    fn adopt_from_sponsor_runs_adopt_then_identify_without_group() {
        let sink = Arc::new(RecordingSink::default());
        let identity = Arc::new(RecordingIdentity::default());
        let facade =
            DefaultAnalyticsFacade::new(sink.clone() as Arc<dyn AnalyticsPort>, identity.clone());

        let id = Uuid::now_v7();
        facade.adopt_from_sponsor(id);

        assert_eq!(identity.adopts.lock().unwrap().as_slice(), &[id]);
        assert_eq!(sink.identifies.lock().unwrap().len(), 1);
        assert!(
            sink.group_identifies.lock().unwrap().is_empty(),
            "joiner / switch path must not redeclare the Space group"
        );
    }

    #[test]
    fn adopt_from_sponsor_skips_identify_when_adopt_fails() {
        let sink = Arc::new(RecordingSink::default());
        let facade = DefaultAnalyticsFacade::new(
            sink.clone() as Arc<dyn AnalyticsPort>,
            Arc::new(AlwaysFailingIdentity),
        );

        facade.adopt_from_sponsor(Uuid::now_v7());
        assert!(sink.identifies.lock().unwrap().is_empty());
    }

    #[test]
    fn release_to_solo_runs_release_then_identify() {
        let sink = Arc::new(RecordingSink::default());
        let identity = Arc::new(RecordingIdentity::default());
        let facade =
            DefaultAnalyticsFacade::new(sink.clone() as Arc<dyn AnalyticsPort>, identity.clone());

        facade.release_to_solo();

        assert_eq!(*identity.releases.lock().unwrap(), 1);
        assert_eq!(sink.identifies.lock().unwrap().len(), 1);
    }

    #[test]
    fn reset_identity_emits_identify_on_success() {
        let sink = Arc::new(RecordingSink::default());
        let identity = Arc::new(RecordingIdentity::default());
        let facade =
            DefaultAnalyticsFacade::new(sink.clone() as Arc<dyn AnalyticsPort>, identity.clone());

        facade.reset_identity().unwrap();

        assert_eq!(*identity.resets.lock().unwrap(), 1);
        assert_eq!(sink.identifies.lock().unwrap().len(), 1);
    }

    #[test]
    fn reset_identity_propagates_error_and_skips_identify() {
        let sink = Arc::new(RecordingSink::default());
        let facade = DefaultAnalyticsFacade::new(
            sink.clone() as Arc<dyn AnalyticsPort>,
            Arc::new(AlwaysFailingIdentity),
        );

        let err = facade.reset_identity().unwrap_err();
        assert!(matches!(err, ResetIdentityError::Storage(_)));
        assert!(sink.identifies.lock().unwrap().is_empty());
    }

    #[test]
    fn noop_facade_methods_are_inert() {
        let f = NoopAnalyticsFacade;
        f.capture(Event::AppFirstOpen);
        f.adopt_self_minted(SelfMintedAdoptRequest {
            space_id: "x".into(),
            now_ms: 0,
        });
        f.adopt_from_sponsor(Uuid::now_v7());
        f.release_to_solo();
        f.reset_identity().unwrap();
        assert!(f.current_space_person_id().is_none());
    }
}
