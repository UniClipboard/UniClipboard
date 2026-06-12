use std::sync::Arc;

use uc_core::{TrustedPeer, TrustedPeerRepositoryPort};

use crate::trusted_peer::errors::TrustedPeerApplicationError;

/// List every peer trusted by this device (DOMAIN §5.2).
///
/// 面向"已信任设备"列表页消费；单空间模型下无需额外输入。
pub struct ListTrustedPeersQuery<R> {
    repository: Arc<R>,
}

impl<R> ListTrustedPeersQuery<R>
where
    R: TrustedPeerRepositoryPort,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self) -> Result<Vec<TrustedPeer>, TrustedPeerApplicationError> {
        Ok(self.repository.list().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_peer::testing::InMemoryTrustedPeerRepository;
    use chrono::Utc;
    use uc_core::security::IdentityFingerprint;
    use uc_core::DeviceId;

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
    async fn list_empty_returns_empty_vec() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let q = ListTrustedPeersQuery::new(repo);
        let result = q.execute().await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn list_returns_every_saved_peer() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        repo.save(&fixture_trusted_peer("a")).await.unwrap();
        repo.save(&fixture_trusted_peer("b")).await.unwrap();

        let q = ListTrustedPeersQuery::new(repo);
        let mut result = q.execute().await.unwrap();
        result.sort_by(|x, y| x.peer_device_id.as_str().cmp(y.peer_device_id.as_str()));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].peer_device_id.as_str(), "a");
        assert_eq!(result[1].peer_device_id.as_str(), "b");
    }
}
