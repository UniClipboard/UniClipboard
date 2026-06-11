use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{info, warn};
use uc_core::security::IdentityFingerprint;
use uc_core::{DeviceId, TrustedPeer, TrustedPeerRepositoryPort};

use crate::trusted_peer::errors::TrustedPeerApplicationError;

/// Input for establishing trust with a peer.
///
/// 典型触发点：用户在 `AwaitingUserVerification` 状态下确认对端身份后，
/// 状态机内部调用本 UseCase 把 `TrustedPeer` 落盘（DOMAIN §5.1）。
#[derive(Debug, Clone)]
pub struct TrustPeer {
    pub local_device_id: DeviceId,
    pub peer_device_id: DeviceId,
    pub peer_fingerprint: IdentityFingerprint,
    pub trusted_at: DateTime<Utc>,
}

/// 登记一个 `TrustedPeer`。
///
/// 重配策略（issue #1023）：同一 `peer_device_id` 已存在时**显式替换**，和
/// `AdmitMemberUseCase` 的策略对称。所有生产调用点都是配对 finalization
/// （sponsor orchestrator / joiner redeem / switch-space），到达这里意味着
/// 本次信任已经被一张新邀请 + passphrase 验证授权——这本身就是"显式重信任"
/// 流程，不是静默覆盖。单向解除配对后对端残留的旧 trust 行因此不再挡死
/// 重新配对。fingerprint 轮换（设备重装/换密钥）会 warn 记录新旧指纹。
pub struct TrustPeerUseCase<R: ?Sized> {
    repository: Arc<R>,
}

impl<R> TrustPeerUseCase<R>
where
    R: TrustedPeerRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        input: TrustPeer,
    ) -> Result<TrustedPeer, TrustedPeerApplicationError> {
        // Re-pair (#1023): a stale row left behind by a one-sided unpair on
        // the other device must not block re-pairing. Reaching this use case
        // is always gated by a freshly verified pairing handshake, so
        // replacing — including a rotated fingerprint — is authorized.
        if let Some(existing) = self.repository.get(&input.peer_device_id).await? {
            if existing.peer_fingerprint != input.peer_fingerprint {
                warn!(
                    peer_device_id = %input.peer_device_id.as_str(),
                    old_fingerprint = %existing.peer_fingerprint,
                    new_fingerprint = %input.peer_fingerprint,
                    "re-trusting peer with rotated identity fingerprint; replacing trust record"
                );
            } else {
                info!(
                    peer_device_id = %input.peer_device_id.as_str(),
                    "re-trusting already-trusted peer; refreshing trust record"
                );
            }
        }

        let peer = TrustedPeer {
            local_device_id: input.local_device_id,
            peer_device_id: input.peer_device_id,
            peer_fingerprint: input.peer_fingerprint,
            trusted_at: input.trusted_at,
        };

        self.repository.save(&peer).await?;
        Ok(peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_peer::testing::InMemoryTrustedPeerRepository;

    fn fp_for(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn fixture(peer_id: &str) -> TrustPeer {
        TrustPeer {
            local_device_id: DeviceId::new("local-1"),
            peer_device_id: DeviceId::new(peer_id),
            peer_fingerprint: fp_for(&format!("FP{peer_id}")),
            trusted_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn first_trust_saves_and_returns_aggregate() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let uc = TrustPeerUseCase::new(repo.clone());

        let saved = uc.execute(fixture("peer-a")).await.unwrap();
        assert_eq!(saved.peer_device_id.as_str(), "peer-a");

        let loaded = repo.get(&saved.peer_device_id).await.unwrap().unwrap();
        assert_eq!(loaded, saved);
    }

    /// Re-pair regression (#1023): a stale trust row left by a one-sided
    /// unpair on the other device must be replaced, not rejected.
    #[tokio::test]
    async fn second_trust_for_same_peer_replaces_record() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let uc = TrustPeerUseCase::new(repo.clone());
        let first = uc.execute(fixture("peer-a")).await.unwrap();

        let mut second_input = fixture("peer-a");
        second_input.trusted_at = first.trusted_at + chrono::Duration::seconds(60);
        let second = uc.execute(second_input).await.unwrap();

        let loaded = repo.get(&second.peer_device_id).await.unwrap().unwrap();
        assert_eq!(loaded, second);
        assert_eq!(
            loaded.trusted_at,
            first.trusted_at + chrono::Duration::seconds(60)
        );
    }

    /// Fingerprint rotation across re-pair (device reinstall / new keys) is
    /// authorized by the fresh handshake; the new fingerprint must win.
    #[tokio::test]
    async fn retrust_with_rotated_fingerprint_replaces_fingerprint() {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let uc = TrustPeerUseCase::new(repo.clone());
        uc.execute(fixture("peer-a")).await.unwrap();

        let mut rotated = fixture("peer-a");
        rotated.peer_fingerprint = fp_for("ROTATEDPEERA");
        let saved = uc.execute(rotated.clone()).await.unwrap();
        assert_eq!(saved.peer_fingerprint, rotated.peer_fingerprint);

        let loaded = repo.get(&DeviceId::new("peer-a")).await.unwrap().unwrap();
        assert_eq!(loaded.peer_fingerprint, rotated.peer_fingerprint);
    }
}
