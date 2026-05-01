//! Bootstrap initialization functions
//!
//! This module contains initialization functions that run during application startup.

use std::sync::Arc;
use uc_application::facade::AppPaths;
use uc_application::facade::SetupStatusFacade;
use uc_core::config::AppConfig;
use uc_core::ids::DeviceId;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::peer_address::PeerAddressRepositoryPort;
use uc_core::ports::{SettingsPort, SetupStatusPort};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_infra::FileSetupStatusRepository;

use crate::assembly::{get_default_app_dirs, get_storage_paths};

/// Returns `true` when the active profile has a completed Space setup.
///
/// Composition-root helper (§3 layer rule: only bootstrap may mix infra
/// adapters + application facades). Wires
/// [`FileSetupStatusRepository`] into the application-layer
/// [`SetupStatusFacade`], so external callers (CLI, GUI, future
/// healthcheck) ask one question through one facade.
///
/// Profile-aware: goes through `get_storage_paths` which applies
/// `apply_profile_suffix` against `UC_PROFILE` env var (set by
/// `uniclipboard-cli`'s `main.rs` from `--profile`), so the same vault
/// dir that `init` / `join` wrote into is the one we read back.
///
/// Legacy fallback: the libp2p-era Tauri `uniclipboard setup` command
/// wrote a `.initialized_encryption` marker file. The domain facade
/// doesn't know about that (Slice 5 will delete the path entirely), so
/// the back-compat check stays here in the composition root as a
/// pragmatic fallback — remove it once nobody has pre-Slice 1 state
/// left.
pub async fn is_setup_complete() -> anyhow::Result<bool> {
    let paths = get_storage_paths(&AppConfig::empty())
        .map_err(|e| anyhow::anyhow!("resolve storage paths: {e}"))?;

    let setup_status: Arc<dyn SetupStatusPort> = Arc::new(
        FileSetupStatusRepository::with_defaults(paths.vault_dir.clone()),
    );
    let facade = SetupStatusFacade::new(setup_status);
    if facade.is_complete().await.unwrap_or(false) {
        return Ok(true);
    }

    // Legacy back-compat only. `SetupStatusFacade::is_complete` is the
    // authoritative Slice 1+ answer.
    let legacy_marker = AppPaths::from_app_dirs(&get_default_app_dirs()?).encryption_marker_path();
    Ok(legacy_marker.exists())
}

/// Ensures the device has a valid name by initializing it with the system hostname if empty.
///
/// When the application starts, this function checks if `device_name` is `None` or an empty
/// string. If so, it fetches the system hostname and saves it as the default device name.
///
/// # Arguments
///
/// * `settings` - A reference to the settings port implementation
///
/// # Returns
///
/// * `Result<(), Box<dyn std::error::Error>>` - Ok on success, error on failure
///
/// # Behavior
///
/// - If `device_name` is `None` or empty, fetches system hostname and saves it
/// - If `device_name` already has a value, does nothing
/// - Logs the initialization event when setting hostname
///
/// # Example
///
/// ```no_run
/// use uc_bootstrap::ensure_default_device_name;
/// use uc_core::ports::SettingsPort;
/// use std::sync::Arc;
///
/// # async fn example(settings: Arc<dyn SettingsPort>) -> Result<(), Box<dyn std::error::Error>> {
/// ensure_default_device_name(settings).await?;
/// # Ok(())
/// # }
/// ```
pub async fn ensure_default_device_name(
    settings: Arc<dyn SettingsPort>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current_settings = settings.load().await?;

    // Check if device_name is None or empty string
    let needs_initialization = current_settings.general.device_name.is_none()
        || current_settings.general.device_name.as_deref() == Some("");

    if needs_initialization {
        let hostname = gethostname::gethostname()
            .to_str()
            .unwrap_or("Uniclipboard Device")
            .to_string();

        tracing::info!("Initializing default device name: {}", hostname);

        current_settings.general.device_name = Some(hostname);
        settings.save(&current_settings).await?;
    }

    Ok(())
}

/// 启动期清理:删除所有"在 `peer_addr_repo` 但不在 `member_repo`"的孤儿
/// 条目。
///
/// `peer_addr_repo` 在文档语义上是"已配对成员的权威集合"(见
/// `dispatch_entry.rs` module doc),`dispatch_entry` 与
/// `ensure_reachable_all` 都直接遍历它来决定 fan-out 目标。Slice 4 P5a-1
/// 之前 unpair 只删 `member_repo`,残留的 peer_addr 条目就成了"已撤销
/// 设备仍被当目标"的根因。Commit A 修了 unpair 路径,但 unpair 时遇到
/// 异常或老版本写入的孤儿,需要这一次启动期 reconcile 兜底清掉。
///
/// 设计取舍:reconcile 失败不会阻断 daemon 启动 —— 失败只 log warn,因为
/// 干净的不变量是"nice to have",运行时即便残留几个孤儿,影响仍然只是
/// "对 unpaired peer 多发几次失败 envelope",不会导致数据损坏。
pub async fn reconcile_peer_addresses(
    member_repo: Arc<dyn MemberRepositoryPort>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
) -> anyhow::Result<()> {
    let members = member_repo
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("list members: {e}"))?;
    // 成员数通常 1–10 量级,linear search 比引入 HashSet 更直接
    // (`DeviceId` 也未实现 Hash)。
    let member_ids: Vec<DeviceId> = members.into_iter().map(|m| m.device_id).collect();

    let peer_addrs = peer_addr_repo
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("list peer addresses: {e}"))?;

    let orphans: Vec<DeviceId> = peer_addrs
        .into_iter()
        .filter_map(|record| {
            if member_ids.contains(&record.device_id) {
                None
            } else {
                Some(record.device_id)
            }
        })
        .collect();

    if orphans.is_empty() {
        tracing::debug!("peer_addr reconcile: no orphans");
        return Ok(());
    }

    tracing::info!(
        orphan_count = orphans.len(),
        "peer_addr reconcile: removing orphan entries (in peer_addr_repo but not in member_repo)"
    );

    for device_id in &orphans {
        match peer_addr_repo.remove(device_id).await {
            Ok(()) => {
                tracing::info!(
                    device_id = %device_id.as_str(),
                    "peer_addr reconcile: removed orphan"
                );
            }
            Err(err) => {
                // 单条失败不阻断其余清理,reconcile 是治理性,不是关键路径。
                tracing::warn!(
                    device_id = %device_id.as_str(),
                    error = %err,
                    "peer_addr reconcile: failed to remove orphan; will retry next boot"
                );
            }
        }
    }

    Ok(())
}

/// 启动期清理:删除所有"在 `trusted_peer_repo` 但不在 `member_repo`"的孤儿
/// 条目。
///
/// 配套 `MemberRosterFacade::revoke_member` 的级联清理:撤销成员时若 trust
/// 删失败,或老版本 unpair 路径根本没碰过 trust 表,残留行会导致**同设备
/// 重新配对**被 `TrustPeerUseCase::execute` 的 `AlreadyTrusted` 检查直接挡死
/// (见 `trust_peer.rs:42`)。reconcile 把不变量 `trusted_peer ⊆ member_repo`
/// 重新拉齐,让重新配对路径不再被历史遗留卡住。
///
/// 跟 `reconcile_peer_addresses` 同样的失败策略:单条 / 整体失败都只 log,
/// 不阻断 daemon 启动。
pub async fn reconcile_trusted_peers(
    member_repo: Arc<dyn MemberRepositoryPort>,
    trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
) -> anyhow::Result<()> {
    let members = member_repo
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("list members: {e}"))?;
    let member_ids: Vec<DeviceId> = members.into_iter().map(|m| m.device_id).collect();

    let trusted = trusted_peer_repo
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("list trusted peers: {e}"))?;

    let orphans: Vec<DeviceId> = trusted
        .into_iter()
        .filter_map(|peer| {
            if member_ids.contains(&peer.peer_device_id) {
                None
            } else {
                Some(peer.peer_device_id)
            }
        })
        .collect();

    if orphans.is_empty() {
        tracing::debug!("trusted_peer reconcile: no orphans");
        return Ok(());
    }

    tracing::info!(
        orphan_count = orphans.len(),
        "trusted_peer reconcile: removing orphan entries (in trusted_peer_repo but not in member_repo)"
    );

    for device_id in &orphans {
        match trusted_peer_repo.remove(device_id).await {
            Ok(removed) => {
                tracing::info!(
                    device_id = %device_id.as_str(),
                    removed,
                    "trusted_peer reconcile: removed orphan"
                );
            }
            Err(err) => {
                tracing::warn!(
                    device_id = %device_id.as_str(),
                    error = %err,
                    "trusted_peer reconcile: failed to remove orphan; will retry next boot"
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex as StdMutex;
    use uc_core::membership::{MemberSyncPreferences, MembershipError, SpaceMember};
    use uc_core::ports::peer_address::{PeerAddressError, PeerAddressRecord};
    use uc_core::security::IdentityFingerprint;
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};

    struct FakeMemberRepo {
        members: Vec<SpaceMember>,
    }

    #[async_trait]
    impl MemberRepositoryPort for FakeMemberRepo {
        async fn get(&self, _device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            unreachable!("reconcile only calls list")
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.members.clone())
        }
        async fn save(&self, _member: &SpaceMember) -> Result<(), MembershipError> {
            unreachable!("reconcile only calls list")
        }
        async fn remove(&self, _device_id: &DeviceId) -> Result<bool, MembershipError> {
            unreachable!("reconcile only calls list")
        }
    }

    struct RecordingPeerAddrRepo {
        records: StdMutex<Vec<PeerAddressRecord>>,
        removed: StdMutex<Vec<DeviceId>>,
        fail_remove_for: Option<DeviceId>,
    }

    #[async_trait]
    impl PeerAddressRepositoryPort for RecordingPeerAddrRepo {
        async fn get(
            &self,
            _device: &DeviceId,
        ) -> Result<Option<PeerAddressRecord>, PeerAddressError> {
            unreachable!("reconcile only calls list + remove")
        }
        async fn upsert(&self, _record: &PeerAddressRecord) -> Result<(), PeerAddressError> {
            unreachable!("reconcile only calls list + remove")
        }
        async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError> {
            Ok(self.records.lock().unwrap().clone())
        }
        async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError> {
            if self
                .fail_remove_for
                .as_ref()
                .is_some_and(|fail| fail == device)
            {
                return Err(PeerAddressError::Internal("disk full".into()));
            }
            self.records
                .lock()
                .unwrap()
                .retain(|r| &r.device_id != device);
            self.removed.lock().unwrap().push(device.clone());
            Ok(())
        }
    }

    fn fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").expect("16-char fingerprint")
    }

    fn member(device: &str) -> SpaceMember {
        SpaceMember {
            device_id: DeviceId::new(device),
            device_name: format!("dev-{device}"),
            identity_fingerprint: fp(),
            joined_at: Utc::now(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    fn peer_record(device: &str) -> PeerAddressRecord {
        PeerAddressRecord {
            device_id: DeviceId::new(device),
            addr_blob: vec![1, 2, 3],
            observed_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn reconcile_removes_only_orphans() {
        let member_repo = Arc::new(FakeMemberRepo {
            members: vec![member("dev-current")],
        });
        let peer_addr_repo = Arc::new(RecordingPeerAddrRepo {
            records: StdMutex::new(vec![
                peer_record("dev-current"),
                peer_record("ebbbd64f-orphan"),
                peer_record("another-ghost"),
            ]),
            removed: StdMutex::new(vec![]),
            fail_remove_for: None,
        });

        reconcile_peer_addresses(member_repo, peer_addr_repo.clone() as _)
            .await
            .expect("reconcile ok");

        let removed = peer_addr_repo.removed.lock().unwrap().clone();
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&DeviceId::new("ebbbd64f-orphan")));
        assert!(removed.contains(&DeviceId::new("another-ghost")));
        // 当前成员不能被误删
        let surviving = peer_addr_repo.records.lock().unwrap();
        assert_eq!(surviving.len(), 1);
        assert_eq!(surviving[0].device_id, DeviceId::new("dev-current"));
    }

    #[tokio::test]
    async fn reconcile_no_op_when_aligned() {
        let member_repo = Arc::new(FakeMemberRepo {
            members: vec![member("a"), member("b")],
        });
        let peer_addr_repo = Arc::new(RecordingPeerAddrRepo {
            records: StdMutex::new(vec![peer_record("a"), peer_record("b")]),
            removed: StdMutex::new(vec![]),
            fail_remove_for: None,
        });

        reconcile_peer_addresses(member_repo, peer_addr_repo.clone() as _)
            .await
            .expect("reconcile ok");

        assert!(peer_addr_repo.removed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reconcile_continues_after_individual_remove_failure() {
        // 一条 remove 失败不应阻断后续清理。
        let member_repo = Arc::new(FakeMemberRepo { members: vec![] });
        let peer_addr_repo = Arc::new(RecordingPeerAddrRepo {
            records: StdMutex::new(vec![peer_record("ghost-a"), peer_record("ghost-b")]),
            removed: StdMutex::new(vec![]),
            fail_remove_for: Some(DeviceId::new("ghost-a")),
        });

        reconcile_peer_addresses(member_repo, peer_addr_repo.clone() as _)
            .await
            .expect("reconcile returns Ok even if a single remove fails");

        // ghost-b 仍被尝试删除并成功
        let removed = peer_addr_repo.removed.lock().unwrap().clone();
        assert_eq!(removed, vec![DeviceId::new("ghost-b")]);
    }

    // ── trusted_peer reconcile ──────────────────────────────────────────

    struct RecordingTrustedPeerRepo {
        rows: StdMutex<Vec<TrustedPeer>>,
        removed: StdMutex<Vec<DeviceId>>,
        fail_remove_for: Option<DeviceId>,
    }

    #[async_trait]
    impl TrustedPeerRepositoryPort for RecordingTrustedPeerRepo {
        async fn get(
            &self,
            _peer_device_id: &DeviceId,
        ) -> Result<Option<TrustedPeer>, TrustedPeerError> {
            unreachable!("reconcile only calls list + remove")
        }
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
            Ok(self.rows.lock().unwrap().clone())
        }
        async fn save(&self, _trusted_peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
            unreachable!("reconcile only calls list + remove")
        }
        async fn remove(&self, peer_device_id: &DeviceId) -> Result<bool, TrustedPeerError> {
            if self
                .fail_remove_for
                .as_ref()
                .is_some_and(|fail| fail == peer_device_id)
            {
                return Err(TrustedPeerError::Repository("io error".into()));
            }
            let mut rows = self.rows.lock().unwrap();
            let before = rows.len();
            rows.retain(|p| &p.peer_device_id != peer_device_id);
            let removed = rows.len() < before;
            self.removed.lock().unwrap().push(peer_device_id.clone());
            Ok(removed)
        }
    }

    fn trusted(device: &str) -> TrustedPeer {
        TrustedPeer {
            local_device_id: DeviceId::new("local"),
            peer_device_id: DeviceId::new(device),
            peer_fingerprint: fp(),
            trusted_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn reconcile_trusted_peers_removes_only_orphans() {
        let member_repo = Arc::new(FakeMemberRepo {
            members: vec![member("active")],
        });
        let trusted_repo = Arc::new(RecordingTrustedPeerRepo {
            rows: StdMutex::new(vec![
                trusted("active"),
                trusted("ghost-a"),
                trusted("ghost-b"),
            ]),
            removed: StdMutex::new(vec![]),
            fail_remove_for: None,
        });

        reconcile_trusted_peers(member_repo, trusted_repo.clone() as _)
            .await
            .expect("reconcile ok");

        let removed = trusted_repo.removed.lock().unwrap().clone();
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&DeviceId::new("ghost-a")));
        assert!(removed.contains(&DeviceId::new("ghost-b")));
        let surviving = trusted_repo.rows.lock().unwrap();
        assert_eq!(surviving.len(), 1);
        assert_eq!(surviving[0].peer_device_id, DeviceId::new("active"));
    }

    #[tokio::test]
    async fn reconcile_trusted_peers_no_op_when_aligned() {
        let member_repo = Arc::new(FakeMemberRepo {
            members: vec![member("a"), member("b")],
        });
        let trusted_repo = Arc::new(RecordingTrustedPeerRepo {
            rows: StdMutex::new(vec![trusted("a"), trusted("b")]),
            removed: StdMutex::new(vec![]),
            fail_remove_for: None,
        });

        reconcile_trusted_peers(member_repo, trusted_repo.clone() as _)
            .await
            .expect("reconcile ok");

        assert!(trusted_repo.removed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reconcile_trusted_peers_continues_after_individual_remove_failure() {
        let member_repo = Arc::new(FakeMemberRepo { members: vec![] });
        let trusted_repo = Arc::new(RecordingTrustedPeerRepo {
            rows: StdMutex::new(vec![trusted("ghost-a"), trusted("ghost-b")]),
            removed: StdMutex::new(vec![]),
            fail_remove_for: Some(DeviceId::new("ghost-a")),
        });

        reconcile_trusted_peers(member_repo, trusted_repo.clone() as _)
            .await
            .expect("reconcile returns Ok even if a single remove fails");

        let removed = trusted_repo.removed.lock().unwrap().clone();
        assert_eq!(removed, vec![DeviceId::new("ghost-b")]);
    }
}
