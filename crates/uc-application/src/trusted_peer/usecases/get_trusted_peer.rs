use std::sync::Arc;

use uc_core::{DeviceId, TrustedPeer, TrustedPeerRepositoryPort};

use crate::trusted_peer::errors::TrustedPeerApplicationError;

/// Input for looking up a single trusted peer by device id.
#[derive(Debug, Clone)]
pub struct GetTrustedPeer {
    pub peer_device_id: DeviceId,
}

/// Return the peer if it is trusted, otherwise `None` (DOMAIN §5.2).
///
/// Deliberately returns `Option` rather than failing with `NotFound`:
/// existence is not a precondition for most callers (e.g. "is this device
/// trusted?" is a legitimate probe).
pub struct GetTrustedPeerQuery<R> {
    repository: Arc<R>,
}

impl<R> GetTrustedPeerQuery<R>
where
    R: TrustedPeerRepositoryPort,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        input: GetTrustedPeer,
    ) -> Result<Option<TrustedPeer>, TrustedPeerApplicationError> {
        Ok(self.repository.get(&input.peer_device_id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_peer::testing::InMemoryTrustedPeerRepository;
    use chrono::Utc;
    use uc_core::security::IdentityFingerprint;

    fn fp_for(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    #[tokio::test]
    async fn missing_peer_returns_none() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let q = GetTrustedPeerQuery::new(repo);

        let result = q
            .execute(GetTrustedPeer {
                peer_device_id: DeviceId::new("missing"),
            })
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn existing_peer_returns_some() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let peer = TrustedPeer {
            local_device_id: DeviceId::new("local-1"),
            peer_device_id: DeviceId::new("peer-a"),
            peer_fingerprint: fp_for("FPA"),
            trusted_at: Utc::now(),
        };
        repo.save(&peer).await.unwrap();

        let q = GetTrustedPeerQuery::new(repo);
        let result = q
            .execute(GetTrustedPeer {
                peer_device_id: DeviceId::new("peer-a"),
            })
            .await
            .unwrap();

        assert_eq!(result, Some(peer));
    }
}
