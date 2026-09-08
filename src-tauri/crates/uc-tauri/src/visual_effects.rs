use crate::visual_effects_storage::{EffectsStorage, StoredEffects, POLICY_VERSION};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const EFFECTS_EVENT: &str = "visual-effects://changed";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EffectsMode {
    Auto,
    Effects,
    Smooth,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AutoResult {
    Effects,
    Smooth,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Capable,
    Constrained,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SystemMotion {
    Reduce,
    Allow,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EffectsReason {
    Manual,
    System,
    PlatformDefault,
    Hardware,
    Unknown,
    Runtime,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EffectsPersistence {
    Saved,
    SessionOnly,
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectsSnapshot {
    pub session_id: String,
    pub revision: u32,
    pub mode: EffectsMode,
    pub auto_for_session: AutoResult,
    pub next_auto: Option<AutoResult>,
    pub system_motion: SystemMotion,
    pub reduce_motion: bool,
    pub low_effects: bool,
    pub reason: EffectsReason,
    pub persistence: EffectsPersistence,
}

#[derive(Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectsSample {
    pub session_id: String,
    pub sample_id: u32,
    pub revision: u32,
    pub frames: u32,
    pub duration_ms: f64,
    pub long_frames: u32,
    pub longest_ms: f64,
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SamplePermit {
    pub sample_id: u32,
    pub revision: u32,
    pub session_id: String,
}

pub struct VisualEffects {
    storage: EffectsStorage,
    stored: StoredEffects,
    linux: bool,
    session_id: String,
    revision: u32,
    auto: AutoResult,
    auto_reason: EffectsReason,
    persistence: EffectsPersistence,
    environments: HashMap<String, SystemMotion>,
    started: Instant,
    last_sample: Option<Instant>,
    permit: Option<(String, SamplePermit, Instant)>,
    sample_count: u32,
    bad_samples: u32,
}

impl VisualEffects {
    pub fn new(
        storage: EffectsStorage,
        mut stored: StoredEffects,
        linux: bool,
        evidence: EvidenceClass,
    ) -> Self {
        if stored.policy_version != POLICY_VERSION || stored.evidence_class != evidence || linux {
            stored.next_auto = None;
        }
        stored.policy_version = POLICY_VERSION;
        stored.evidence_class = evidence;
        let (auto, auto_reason) = if linux {
            (AutoResult::Smooth, EffectsReason::PlatformDefault)
        } else if let Some(next) = stored.next_auto {
            (next, EffectsReason::Runtime)
        } else {
            match evidence {
                EvidenceClass::Capable => (AutoResult::Effects, EffectsReason::Hardware),
                EvidenceClass::Constrained => (AutoResult::Smooth, EffectsReason::Hardware),
                EvidenceClass::Unknown => (AutoResult::Smooth, EffectsReason::Unknown),
            }
        };
        let persistence = if storage.writable() {
            EffectsPersistence::Saved
        } else {
            EffectsPersistence::SessionOnly
        };
        Self {
            storage,
            stored,
            linux,
            session_id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            auto,
            auto_reason,
            persistence,
            environments: HashMap::new(),
            started: Instant::now(),
            last_sample: None,
            permit: None,
            sample_count: 0,
            bad_samples: 0,
        }
    }

    pub fn snapshot(&self) -> EffectsSnapshot {
        let system = if self
            .environments
            .values()
            .any(|v| *v == SystemMotion::Reduce)
        {
            SystemMotion::Reduce
        } else if self.environments.is_empty()
            || self
                .environments
                .values()
                .any(|v| *v == SystemMotion::Unknown)
        {
            SystemMotion::Unknown
        } else {
            SystemMotion::Allow
        };
        let system_reduced = system != SystemMotion::Allow;
        let low = match self.stored.mode {
            EffectsMode::Smooth => true,
            EffectsMode::Effects => false,
            EffectsMode::Auto => system_reduced || self.auto == AutoResult::Smooth,
        };
        EffectsSnapshot {
            session_id: self.session_id.clone(),
            revision: self.revision,
            mode: self.stored.mode,
            auto_for_session: self.auto,
            next_auto: self.stored.next_auto,
            system_motion: system,
            reduce_motion: system_reduced || low,
            low_effects: low,
            reason: if system_reduced {
                EffectsReason::System
            } else if self.stored.mode != EffectsMode::Auto {
                EffectsReason::Manual
            } else {
                self.auto_reason
            },
            persistence: self.persistence,
        }
    }

    fn invalidate(&mut self) {
        self.revision += 1;
        self.permit = None;
        self.bad_samples = 0;
    }

    pub fn set_mode(&mut self, mode: EffectsMode) {
        if self.stored.mode != mode {
            self.stored.mode = mode;
            self.invalidate();
        }
        self.save();
    }

    pub fn environment(&mut self, window: &str, session: &str, motion: SystemMotion) {
        if session != self.session_id {
            return;
        }
        if self.environments.get(window) != Some(&motion) {
            self.environments.insert(window.to_owned(), motion);
            self.invalidate();
        }
    }

    pub fn remove_window(&mut self, window: &str) {
        if self.environments.remove(window).is_some() {
            self.invalidate();
        }
    }

    fn save(&mut self) {
        self.persistence = match self.storage.save(&self.stored) {
            Ok(()) => EffectsPersistence::Saved,
            Err(error) => {
                tracing::warn!(error_kind = "visual_effects_write", source = %error, "visual preferences apply for this session only");
                EffectsPersistence::SessionOnly
            }
        };
        self.revision += 1;
    }

    fn can_sample(&self) -> bool {
        let snapshot = self.snapshot();
        !self.linux
            && snapshot.mode == EffectsMode::Auto
            && !snapshot.reduce_motion
            && self.stored.next_auto != Some(AutoResult::Smooth)
    }

    pub fn begin_sample(
        &mut self,
        window: &str,
        session: &str,
        now: Instant,
    ) -> Option<SamplePermit> {
        if session != self.session_id
            || !self.can_sample()
            || self.sample_count >= 10
            || now.duration_since(self.started) < Duration::from_secs(10)
            || self
                .last_sample
                .is_some_and(|last| now.duration_since(last) < Duration::from_secs(30))
        {
            return None;
        }
        self.sample_count += 1;
        self.last_sample = Some(now);
        let permit = SamplePermit {
            sample_id: self.sample_count,
            revision: self.revision,
            session_id: self.session_id.clone(),
        };
        self.permit = Some((window.to_owned(), permit.clone(), now));
        Some(permit)
    }

    pub fn report_sample(&mut self, window: &str, sample: EffectsSample, now: Instant) {
        let Some((owner, permit, start)) = &self.permit else {
            return;
        };
        if owner != window
            || sample.session_id != self.session_id
            || sample.sample_id != permit.sample_id
            || sample.revision != self.revision
            || sample.revision != permit.revision
        {
            return;
        }
        // Allow IPC delivery time, but never count a delayed/resumed window's report.
        let timely = now.duration_since(*start) <= Duration::from_secs(3);
        self.permit = None;
        if !timely
            || !self.can_sample()
            || sample.frames < 30
            || sample.frames > 2000
            || sample.long_frames > sample.frames
            || !sample.duration_ms.is_finite()
            || sample.duration_ms <= 0.0
            || sample.duration_ms > 2000.0
            || !sample.longest_ms.is_finite()
            || sample.longest_ms <= 0.0
            || sample.longest_ms > 500.0
            || sample.longest_ms > sample.duration_ms
            || f64::from(sample.long_frames) * 50.0 > sample.duration_ms
        {
            return;
        }
        if sample.long_frames >= 6
            && f64::from(sample.long_frames) / f64::from(sample.frames) >= 0.2
        {
            self.bad_samples += 1;
            if self.bad_samples >= 3 {
                self.stored.next_auto = Some(AutoResult::Smooth);
                self.save();
            }
        }
    }
}

pub struct VisualEffectsService(pub tokio::sync::OnceCell<tokio::sync::Mutex<VisualEffects>>);
impl Default for VisualEffectsService {
    fn default() -> Self {
        Self(tokio::sync::OnceCell::new())
    }
}
impl VisualEffectsService {
    pub async fn get(&self) -> &tokio::sync::Mutex<VisualEffects> {
        self.0
            .get_or_init(|| async {
                let evidence = crate::visual_effects_probe::probe().await;
                let path =
                    uc_app_paths::app_data_root().map(|root| root.join("visual-effects.json"));
                let (storage, stored) = EffectsStorage::load(path);
                tokio::sync::Mutex::new(VisualEffects::new(
                    storage,
                    stored,
                    cfg!(target_os = "linux"),
                    evidence,
                ))
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(linux: bool, evidence: EvidenceClass) -> VisualEffects {
        let (storage, stored) = EffectsStorage::load(None);
        let mut s = VisualEffects::new(storage, stored, linux, evidence);
        s.environment("main", &s.session_id.clone(), SystemMotion::Allow);
        s
    }
    #[test]
    fn visual_effects_linux_and_system_priority() {
        for evidence in [
            EvidenceClass::Capable,
            EvidenceClass::Constrained,
            EvidenceClass::Unknown,
        ] {
            let mut s = state(true, evidence);
            assert!(s.snapshot().low_effects);
            s.set_mode(EffectsMode::Effects);
            assert!(!s.snapshot().reduce_motion);
            s.environment("main", &s.session_id.clone(), SystemMotion::Reduce);
            assert!(s.snapshot().reduce_motion);
            assert!(!s.snapshot().low_effects);
            s.set_mode(EffectsMode::Auto);
            assert!(s.snapshot().low_effects);
            assert!(s
                .begin_sample(
                    "main",
                    &s.session_id.clone(),
                    s.started + Duration::from_secs(20)
                )
                .is_none());
        }
    }
    #[test]
    fn visual_effects_three_reports_only_change_next_start() {
        let mut s = state(false, EvidenceClass::Capable);
        for i in 0..3 {
            let now = s.started + Duration::from_secs(15 + i * 31);
            let p = s.begin_sample("main", &s.session_id.clone(), now).unwrap();
            assert!(s
                .begin_sample("panel", &s.session_id.clone(), now)
                .is_none());
            s.report_sample(
                "main",
                EffectsSample {
                    session_id: p.session_id,
                    sample_id: p.sample_id,
                    revision: p.revision,
                    frames: 40,
                    duration_ms: 1900.0,
                    long_frames: 10,
                    longest_ms: 90.0,
                },
                now + Duration::from_secs(2),
            );
            assert!(!s.snapshot().low_effects);
            assert_eq!(
                s.snapshot().next_auto,
                if i == 2 {
                    Some(AutoResult::Smooth)
                } else {
                    None
                }
            );
        }
        let (storage, _) = EffectsStorage::load(None);
        let restarted =
            VisualEffects::new(storage, s.stored.clone(), false, EvidenceClass::Capable);
        assert_eq!(restarted.auto, AutoResult::Smooth);
    }
    #[test]
    fn visual_effects_snapshot_uses_camel_case() {
        let value = serde_json::to_value(state(false, EvidenceClass::Unknown).snapshot()).unwrap();
        assert!(value.get("reduceMotion").is_some());
        assert!(value.get("reduce_motion").is_none());
    }

    #[test]
    fn visual_effects_reclassifies_old_unknown_policy_without_changing_user_choice() {
        for mode in [EffectsMode::Auto, EffectsMode::Effects, EffectsMode::Smooth] {
            let (storage, mut stored) = EffectsStorage::load(None);
            stored.mode = mode;
            stored.policy_version = 1;
            stored.evidence_class = EvidenceClass::Unknown;
            stored.next_auto = Some(AutoResult::Smooth);
            let mut s = VisualEffects::new(storage, stored, false, EvidenceClass::Capable);
            s.environment("main", &s.session_id.clone(), SystemMotion::Allow);
            let snapshot = s.snapshot();
            assert_eq!(snapshot.mode, mode);
            assert_eq!(snapshot.auto_for_session, AutoResult::Effects);
            assert_eq!(snapshot.next_auto, None);
            assert_eq!(snapshot.low_effects, mode == EffectsMode::Smooth);
        }
    }
    #[test]
    fn visual_effects_mode_system_matrix() {
        for linux in [false, true] {
            for evidence in [
                EvidenceClass::Capable,
                EvidenceClass::Constrained,
                EvidenceClass::Unknown,
            ] {
                for mode in [EffectsMode::Auto, EffectsMode::Effects, EffectsMode::Smooth] {
                    for motion in [
                        SystemMotion::Allow,
                        SystemMotion::Reduce,
                        SystemMotion::Unknown,
                    ] {
                        let mut s = state(linux, evidence);
                        s.set_mode(mode);
                        s.environment("main", &s.session_id.clone(), motion);
                        let low = mode == EffectsMode::Smooth
                            || (mode == EffectsMode::Auto
                                && (linux
                                    || evidence != EvidenceClass::Capable
                                    || motion != SystemMotion::Allow));
                        assert_eq!(s.snapshot().low_effects, low);
                        assert_eq!(
                            s.snapshot().reduce_motion,
                            low || motion != SystemMotion::Allow
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn visual_effects_manual_switch_invalidates_inflight_report() {
        let mut s = state(false, EvidenceClass::Capable);
        let now = s.started + Duration::from_secs(20);
        let permit = s.begin_sample("main", &s.session_id.clone(), now).unwrap();
        s.set_mode(EffectsMode::Effects);
        s.report_sample(
            "main",
            EffectsSample {
                session_id: permit.session_id,
                sample_id: permit.sample_id,
                revision: permit.revision,
                frames: 40,
                duration_ms: 1900.0,
                long_frames: 10,
                longest_ms: 90.0,
            },
            now,
        );
        assert_eq!(s.bad_samples, 0);
        assert_eq!(s.snapshot().mode, EffectsMode::Effects);
    }
    #[test]
    fn visual_effects_environment_union_and_destroyed_window() {
        let mut s = state(false, EvidenceClass::Capable);
        s.environment("panel", &s.session_id.clone(), SystemMotion::Reduce);
        assert!(s.snapshot().reduce_motion);
        s.remove_window("panel");
        assert!(!s.snapshot().reduce_motion);
        assert_eq!(s.snapshot().auto_for_session, AutoResult::Effects);
    }
    #[test]
    fn visual_effects_linux_ignores_old_effects_override() {
        let (storage, mut stored) = EffectsStorage::load(None);
        stored.next_auto = Some(AutoResult::Effects);
        let s = VisualEffects::new(storage, stored, true, EvidenceClass::Capable);
        assert_eq!(s.snapshot().auto_for_session, AutoResult::Smooth);
        assert_eq!(s.snapshot().next_auto, None);
    }
}
