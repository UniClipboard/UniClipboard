use std::sync::Arc;

use uc_core::{DeviceId, TrustedPeerRepositoryPort};

use crate::trusted_peer::errors::TrustedPeerApplicationError;

/// Input for revoking trust with a peer.
#[derive(Debug, Clone)]
pub struct DistrustPeer {
    pub peer_device_id: DeviceId,
}

/// 撤销对一台设备的信任（硬删）。
///
/// 不连带撤销成员关系 —— DOMAIN §9.6 / T8：`DistrustPeerUseCase` 与
/// `RevokeMemberUseCase` 是两个独立 UseCase，UI 的"解除配对"由 Facade
/// 级联调用。
pub struct DistrustPeerUseCase<R> {
    repository: Arc<R>,
}

impl<R> DistrustPeerUseCase<R>
where
    R: TrustedPeerRepositoryPort,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, input: DistrustPeer) -> Result<(), TrustedPeerApplicationError> {
        if !self.repository.remove(&input.peer_device_id).await? {
            return Err(TrustedPeerApplicationError::NotFound(input.peer_device_id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_peer::testing::InMemoryTrustedPeerRepository;
    use chrono::Utc;
    use uc_core::security::IdentityFingerprint;
    use uc_core::TrustedPeer;

    fn fp_for(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn fixture_trusted_peer(peer_id: &str) -> TrustedPeer {
        TrustedPeer {
            local_device_id: DeviceId::new("local-1"),
            peer_device_id: DeviceId::new(peer_id),
            peer_fingerprint: fp_for(&format!("FP{peer_id}")),
            trusted_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn distrust_removes_existing_peer() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        repo.save(&fixture_trusted_peer("peer-a")).await.unwrap();

        let uc = DistrustPeerUseCase::new(repo.clone());
        uc.execute(DistrustPeer {
            peer_device_id: DeviceId::new("peer-a"),
        })
        .await
        .unwrap();

        assert!(repo.get(&DeviceId::new("peer-a")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn distrust_missing_peer_returns_not_found() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let uc = DistrustPeerUseCase::new(repo);

        let err = uc
            .execute(DistrustPeer {
                peer_device_id: DeviceId::new("missing"),
            })
            .await
            .unwrap_err();

        assert_eq!(
            err,
            TrustedPeerApplicationError::NotFound(DeviceId::new("missing"))
        );
    }
}
