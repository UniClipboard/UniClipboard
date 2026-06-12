use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::info;
use uc_core::security::IdentityFingerprint;
use uc_core::{DeviceId, MemberRepositoryPort, MemberSyncPreferences, SpaceMember};

use crate::membership::errors::MembershipApplicationError;

/// Input for admitting a new member.
///
/// 典型触发点：`space_access` 完成后，由上层流程把对端设备登记进本机成员列表。
/// 本机成员关系是**本地自治**的，不会广播给其他设备。
#[derive(Debug, Clone)]
pub struct AdmitMember {
    pub device_id: DeviceId,
    pub device_name: String,
    pub identity_fingerprint: IdentityFingerprint,
    pub joined_at: DateTime<Utc>,
    pub sync_preferences: MemberSyncPreferences,
}

/// 将新设备接纳为本机空间成员。
///
/// 重配策略（issue #1023）：同一 `device_id` 重复 admit 视为重新配对，
/// **显式替换**既有记录而不是报错——单向解除配对后对端残留的旧 member
/// 行不能挡死重新配对。替换时保留既有 `sync_preferences`（用户对该设备
/// 的本地同步配置不因重配丢失），name / fingerprint / joined_at 取新值。
/// 和 `TrustPeerUseCase` 的策略对称。
pub struct AdmitMemberUseCase<R: ?Sized> {
    repository: Arc<R>,
}

impl<R> AdmitMemberUseCase<R>
where
    R: MemberRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        input: AdmitMember,
    ) -> Result<SpaceMember, MembershipApplicationError> {
        // Re-pair (#1023): a stale row left behind by a one-sided unpair on
        // the other device must not block re-admitting the same device.
        // Keep the user's local sync preferences for it; everything else
        // comes from the fresh handshake.
        let sync_preferences = match self.repository.get(&input.device_id).await? {
            Some(existing) => {
                info!(
                    device_id = %input.device_id.as_str(),
                    "re-admitting known device; replacing stale member record \
                     (sync preferences preserved)"
                );
                existing.sync_preferences
            }
            None => input.sync_preferences,
        };

        let member = SpaceMember {
            device_id: input.device_id,
            device_name: input.device_name,
            identity_fingerprint: input.identity_fingerprint,
            joined_at: input.joined_at,
            sync_preferences,
        };

        self.repository.save(&member).await?;
        Ok(member)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use uc_core::settings::model::ContentTypes;
    use uc_core::MembershipError;

    #[derive(Default)]
    struct InMemoryMemberRepo {
        inner: Mutex<HashMap<String, SpaceMember>>,
    }

    #[async_trait::async_trait]
    impl MemberRepositoryPort for InMemoryMemberRepo {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().unwrap().get(device_id.as_str()).cloned())
        }

        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().unwrap().values().cloned().collect())
        }

        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
            self.inner
                .lock()
                .unwrap()
                .insert(member.device_id.as_str().to_string(), member.clone());
            Ok(())
        }

        async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .remove(device_id.as_str())
                .is_some())
        }
    }

    fn fp_for(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn fixture(device_id: &str) -> AdmitMember {
        AdmitMember {
            device_id: DeviceId::new(device_id),
            device_name: format!("{device_id}-name"),
            identity_fingerprint: fp_for(&format!("FP{device_id}")),
            joined_at: Utc::now(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    #[tokio::test]
    async fn first_admit_saves_member() {
        let repo = Arc::new(InMemoryMemberRepo::default());
        let uc = AdmitMemberUseCase::new(repo.clone());

        let saved = uc.execute(fixture("dev-a")).await.unwrap();
        assert_eq!(saved.device_id.as_str(), "dev-a");

        let loaded = repo.get(&saved.device_id).await.unwrap().unwrap();
        assert_eq!(loaded, saved);
    }

    /// Re-pair regression (#1023): a stale row left by a one-sided unpair on
    /// the other device must be replaced, not rejected with `AlreadyAdmitted`.
    #[tokio::test]
    async fn re_admit_replaces_record_and_preserves_sync_preferences() {
        let repo = Arc::new(InMemoryMemberRepo::default());
        let uc = AdmitMemberUseCase::new(repo.clone());
        let first = uc.execute(fixture("dev-a")).await.unwrap();

        // Simulate a user-customized preference before the re-pair.
        let customized = SpaceMember {
            sync_preferences: MemberSyncPreferences {
                send_enabled: false,
                receive_enabled: true,
                send_content_types: ContentTypes::default(),
                receive_content_types: ContentTypes::default(),
            },
            ..first
        };
        repo.save(&customized).await.unwrap();

        let mut re_admit = fixture("dev-a");
        re_admit.device_name = "dev-a-renamed".into();
        re_admit.identity_fingerprint = fp_for("ROTATEDDEVA");
        re_admit.joined_at = customized.joined_at + chrono::Duration::seconds(60);

        let replaced = uc.execute(re_admit.clone()).await.unwrap();

        // Fresh handshake facts win …
        assert_eq!(replaced.device_name, "dev-a-renamed");
        assert_eq!(replaced.identity_fingerprint, re_admit.identity_fingerprint);
        assert_eq!(replaced.joined_at, re_admit.joined_at);
        // … but the local sync preferences survive the re-pair.
        assert!(!replaced.sync_preferences.send_enabled);

        let loaded = repo.get(&replaced.device_id).await.unwrap().unwrap();
        assert_eq!(loaded, replaced);
    }
}
