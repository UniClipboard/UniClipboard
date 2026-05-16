//! `SwitchSpaceUseCase` — 已 setup 设备加入另一个 sponsor 空间的 4 阶段
//! 重加密迁移。
//!
//! ## 为什么需要它
//!
//! `RedeemPairingInvitationUseCase` 假设设备处于"未 setup"状态——它会无条件
//! 把 `setup_status.has_completed` 翻成 true，且 `derive_master_key_for_proof`
//! 会立即覆写 keyring KEK / 磁盘 keyslot / 内存 session。这导致已经有本地
//! 剪贴板历史的设备没法用 redeem 流程加入新空间，因为：
//!
//! * setup_status 会冲突（已经 true）。
//! * 历史剪贴板条目用旧 master_key 加密，handshake 之后旧 key 三处都丢了，
//!   主表立刻不可读。
//!
//! 本 use case 通过"四阶段迁移 + 备份表 + 一次性 migration_key"把数据
//! 从旧 master_key 转加密到新 master_key，期间任意一步崩溃都能在下次启动
//! 时按当前 [`MigrationPhase`] 续跑。
//!
//! ## 阶段流程
//!
//! 1. **Phase 1 (prepare)** — 用旧 session master_key 解密所有 inline
//!    representation 主表行，再用刚生成的 migration_key 重加密，落到
//!    `clipboard_migration_backup` 表。完成时 `migration_state` =
//!    `Prepared { run_id }`。失败：清空 backup 表 + 销毁 migration_key，
//!    旧空间数据完整。
//!
//! 2. **Phase 2 (handshake & persist)** — 调 [`JoinerHandshakeRunner`]，
//!    handshake 内部会把 keyring/磁盘/session 三处的 master_key 换成新
//!    空间的；之后落地 sponsor 的 `SpaceMember` / `TrustedPeer` /
//!    `PeerAddressRecord`。完成时 `migration_state` =
//!    `HandshakeDone { run_id, target_space_id }`。失败：清空 backup +
//!    销毁 migration_key（旧 master_key 仍在 session/磁盘/keyring，所以
//!    旧空间还能继续用——除非 handshake 的 derive_master_key_for_proof
//!    已经覆写了，那种情况下设备需要手动 factory_reset，本流程已无能为力）。
//!
//! 3. **Phase 3 (swap)** — 从 backup 表流式读出 migration-key 加密的密文，
//!    解密成明文，用新 session master_key 重新加密，覆写主表对应 row。
//!    完成时 `migration_state` = `Swapped { run_id, target_space_id }`。
//!    失败：报错给用户，状态留 `HandshakeDone`，下次启动重试。
//!
//! 4. **Phase 4 (commit)** — `setup_status.space_id` 切到新 space_id，
//!    按持久化的 `sponsor_space_person_id` 切换 telemetry 身份
//!    （adopt 新 person / 回退 Solo），清空 backup 表，销毁
//!    migration_key，`migration_state` = `None`。身份切换的*意图*在
//!    阶段 2 写 `HandshakeDone` 时就已落盘，因此即便 commit 与
//!    identify 之间崩溃，下次启动 resume 仍能补做。
//!
//! ## 启动期续跑
//!
//! [`Self::resume_pending`] 在 daemon 启动时（commit 4 接入 facade）按
//! 当前 [`MigrationPhase`] 决定动作：
//!
//! * `None` — 不做任何事（无在飞迁移）。
//! * `Prepared` — 视为放弃：清空 backup + 销毁 migration_key，回到 None。
//! * `HandshakeDone` — 重跑 phase 3 + phase 4。
//! * `Swapped` — 仅重跑 phase 4 cleanup。
//!
//! ## 不属于本 use case 的关切
//!
//! * 暂停 `ClipboardWatcher`：迁移期间用户复制的新内容会被 watcher 用当前
//!   session master_key 加密（取决于 phase 时点：phase 1-2 期间还是旧 key，
//!   phase 3 之后是新 key）。短暂的"phase 2 内 master_key 已切但 phase 3
//!   未跑完"窗口里写入的新条目可能落在主表但 backup 表已固定、phase 3
//!   不会动它——因为是用新 key 写的，本来就用新 key 能读。所以即使不暂停
//!   watcher 也不会丢数据。后续 commit 可以加暂停以减小不一致窗口。
//! * 进度反馈：本 use case 同步执行；进度查询走独立路径
//!   `BlobMigrationRepoPort::count_records()`（commit 5 暴露成 HTTP 路由）。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::{debug, info, instrument, warn};

use uc_core::crypto::aad;
use uc_core::crypto::domain::{Aad, ActiveSpace, Ciphertext, Passphrase};
use uc_core::ids::SpaceId;
use uc_core::membership::{MemberRepositoryPort, MemberSyncPreferences};
use uc_core::pairing::invitation::InvitationCode;
use uc_core::ports::clipboard::{BlobMigrationRepoError, BlobMigrationRepoPort, MigrationRecord};
use uc_core::ports::security::{
    BlobCipherError, BlobCipherPort, KeyMigrationError, KeyMigrationPort,
};
use uc_core::ports::setup::{MigrationStateError, MigrationStatePort};
use uc_core::ports::{ClockPort, PeerAddressRecord, PeerAddressRepositoryPort, SetupStatusPort};
use uc_core::setup::{MigrationPhase, MigrationRunId, SetupStatus};
use uc_core::TrustedPeerRepositoryPort;
use uc_observability::analytics::AnalyticsFacade;
use uuid::Uuid;

use crate::facade::space_setup::commands::SwitchSpaceCommand;
use crate::facade::space_setup::{
    RedeemPairingInvitationError, SwitchSpaceError, SwitchSpaceResult,
};
use crate::membership::errors::MembershipApplicationError;
use crate::membership::usecases::{AdmitMember, AdmitMemberUseCase};
use crate::pairing_outbound::joiner_handshake::{
    JoinerHandshakeCoordinator, JoinerHandshakeOutcome,
};
use crate::trusted_peer::errors::TrustedPeerApplicationError;
use crate::trusted_peer::usecases::{TrustPeer, TrustPeerUseCase};

pub(crate) type AdmitMemberUc = AdmitMemberUseCase<dyn MemberRepositoryPort>;
pub(crate) type TrustPeerUc = TrustPeerUseCase<dyn TrustedPeerRepositoryPort>;

// ---------------------------------------------------------------------------
// HandshakeRunner —— 让 use case 不直接持有 `Arc<JoinerHandshakeCoordinator>`，
// 测试时可以 mockall 替换。生产路径下由 facade 注入对真 coordinator 的薄
// wrapper（见底部 `impl JoinerHandshakeRunner for JoinerHandshakeCoordinator`）。
// ---------------------------------------------------------------------------

/// 把 `JoinerHandshakeCoordinator::handshake` 抽象为单方法 trait——只有
/// switch-space use case 需要这一层间接，让单元测试能用 `mockall` 隔离
/// handshake 的 wire+crypto 子图。
#[async_trait]
pub(crate) trait JoinerHandshakeRunner: Send + Sync {
    async fn run(
        &self,
        code: &InvitationCode,
        passphrase: &Passphrase,
    ) -> Result<JoinerHandshakeOutcome, RedeemPairingInvitationError>;
}

#[async_trait]
impl JoinerHandshakeRunner for JoinerHandshakeCoordinator {
    async fn run(
        &self,
        code: &InvitationCode,
        passphrase: &Passphrase,
    ) -> Result<JoinerHandshakeOutcome, RedeemPairingInvitationError> {
        self.handshake(code, passphrase).await
    }
}

// ---------------------------------------------------------------------------
// SwitchSpaceUseCase
// ---------------------------------------------------------------------------
//
// 应用层 Command / Result / Error 由 facade 层（`facade::space_setup::commands`
// + `facade::space_setup::errors`）拥有，与 redeem use case 的 pattern 对齐。
// 本模块只持有编排逻辑，类型从 facade 引入。

pub(crate) struct SwitchSpaceUseCase {
    setup_status: Arc<dyn SetupStatusPort>,
    migration_state: Arc<dyn MigrationStatePort>,
    key_migration: Arc<dyn KeyMigrationPort>,
    blob_migration_repo: Arc<dyn BlobMigrationRepoPort>,
    blob_cipher: Arc<dyn BlobCipherPort>,
    handshake: Arc<dyn JoinerHandshakeRunner>,
    admit_member: Arc<AdmitMemberUc>,
    trust_peer: Arc<TrustPeerUc>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    /// Switches the local analytics identity to the target Space's
    /// person once commit phase has succeeded. `None` on the target
    /// sponsor side falls back to Solo so cross-Space switches never
    /// strand the device on the old person.
    analytics: Arc<dyn AnalyticsFacade>,
}

impl SwitchSpaceUseCase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        setup_status: Arc<dyn SetupStatusPort>,
        migration_state: Arc<dyn MigrationStatePort>,
        key_migration: Arc<dyn KeyMigrationPort>,
        blob_migration_repo: Arc<dyn BlobMigrationRepoPort>,
        blob_cipher: Arc<dyn BlobCipherPort>,
        handshake: Arc<dyn JoinerHandshakeRunner>,
        admit_member: Arc<AdmitMemberUc>,
        trust_peer: Arc<TrustPeerUc>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        clock: Arc<dyn ClockPort>,
        analytics: Arc<dyn AnalyticsFacade>,
    ) -> Self {
        Self {
            setup_status,
            migration_state,
            key_migration,
            blob_migration_repo,
            blob_cipher,
            handshake,
            admit_member,
            trust_peer,
            peer_addr_repo,
            clock,
            analytics,
        }
    }

    #[instrument(skip_all, fields(code = %cmd.code.as_str()))]
    pub(crate) async fn execute(
        &self,
        cmd: SwitchSpaceCommand,
    ) -> Result<SwitchSpaceResult, SwitchSpaceError> {
        // ── Pre-flight ────────────────────────────────────────────────────
        let status = self
            .setup_status
            .get_status()
            .await
            .map_err(|e| SwitchSpaceError::Storage(e.to_string()))?;
        if !status.has_completed {
            return Err(SwitchSpaceError::NotSetup);
        }
        if let Some(existing) = self
            .migration_state
            .get_current()
            .await
            .map_err(map_migration_state_err)?
        {
            return Err(SwitchSpaceError::PendingMigration(existing));
        }

        // ── Phase 1 — prepare backup ─────────────────────────────────────
        let run_id = match self.phase_1_prepare().await {
            Ok(id) => id,
            Err(err) => {
                self.cleanup_after_phase1_failure().await;
                return Err(err);
            }
        };
        self.migration_state
            .set_current(Some(MigrationPhase::Prepared {
                run_id: run_id.clone(),
            }))
            .await
            .map_err(map_migration_state_err)?;
        info!(run_id = %run_id, "switch-space phase 1 (prepare) complete");

        // ── Phase 2 — handshake + admit + trust + peer-addr ──────────────
        let outcome = match self
            .phase_2_handshake_and_persist(&cmd.code, &cmd.new_passphrase)
            .await
        {
            Ok(o) => o,
            Err(err) => {
                self.cleanup_after_phase2_failure(&run_id).await;
                return Err(err);
            }
        };
        let target_space_id = outcome.space_id.clone();
        let identity_target = outcome.sponsor_space_person_id;
        self.migration_state
            .set_current(Some(MigrationPhase::HandshakeDone {
                run_id: run_id.clone(),
                target_space_id: target_space_id.clone(),
                sponsor_space_person_id: identity_target,
            }))
            .await
            .map_err(map_migration_state_err)?;
        info!(
            run_id = %run_id,
            target_space_id = %target_space_id,
            "switch-space phase 2 (handshake & persist) complete"
        );

        // ── Phase 3 — swap main table ────────────────────────────────────
        let migrated_records = self
            .blob_migration_repo
            .count_records()
            .await
            .map_err(map_blob_repo_err)?;
        self.phase_3_swap(&run_id).await?;
        self.migration_state
            .set_current(Some(MigrationPhase::Swapped {
                run_id: run_id.clone(),
                target_space_id: target_space_id.clone(),
                sponsor_space_person_id: identity_target,
            }))
            .await
            .map_err(map_migration_state_err)?;
        info!(
            run_id = %run_id,
            target_space_id = %target_space_id,
            migrated = migrated_records,
            "switch-space phase 3 (swap) complete"
        );

        // ── Phase 4 — finalise + cleanup ─────────────────────────────────
        // identity_target 已经在阶段 2/3 落进 migration_state，phase_4_commit
        // 内部负责按这个意图切换 telemetry person——这样 daemon 在 commit
        // 与 identify 之间崩溃后，下次启动 resume_pending 续跑 phase 4 时
        // 仍能从持久化状态里还原意图并补做切换。
        self.phase_4_commit(&run_id, &target_space_id, identity_target)
            .await?;

        Ok(SwitchSpaceResult {
            sponsor_device_id: outcome.sponsor_device_id,
            sponsor_identity_fingerprint: outcome.sponsor_identity_fingerprint,
            space_id: target_space_id,
            self_device_id: outcome.self_device_id,
            self_identity_fingerprint: outcome.self_identity_fingerprint,
            migrated_records,
        })
    }

    /// 启动期补偿：按当前 `MigrationPhase` 续跑或清理。
    ///
    /// 注：本方法不接受 passphrase 参数——`HandshakeDone` 续跑只需新空间
    /// 当前 session 的 master_key（由 `try_resume_session` 在 daemon 启动
    /// 时通过 keyring KEK 静默解锁），不需要重新走 handshake。
    pub(crate) async fn resume_pending(&self) -> Result<(), SwitchSpaceError> {
        let phase = match self
            .migration_state
            .get_current()
            .await
            .map_err(map_migration_state_err)?
        {
            None => return Ok(()),
            Some(p) => p,
        };
        match phase {
            MigrationPhase::Prepared { run_id } => {
                warn!(
                    run_id = %run_id,
                    "found stranded Prepared migration; aborting and cleaning up"
                );
                self.cleanup_after_phase2_failure(&run_id).await;
                Ok(())
            }
            MigrationPhase::HandshakeDone {
                run_id,
                target_space_id,
                sponsor_space_person_id,
            } => {
                info!(
                    run_id = %run_id,
                    target_space_id = %target_space_id,
                    "resuming HandshakeDone migration: replay phase 3 + phase 4"
                );
                self.phase_3_swap(&run_id).await?;
                self.migration_state
                    .set_current(Some(MigrationPhase::Swapped {
                        run_id: run_id.clone(),
                        target_space_id: target_space_id.clone(),
                        sponsor_space_person_id,
                    }))
                    .await
                    .map_err(map_migration_state_err)?;
                self.phase_4_commit(&run_id, &target_space_id, sponsor_space_person_id)
                    .await?;
                Ok(())
            }
            MigrationPhase::Swapped {
                run_id,
                target_space_id,
                sponsor_space_person_id,
            } => {
                info!(
                    run_id = %run_id,
                    target_space_id = %target_space_id,
                    "resuming Swapped migration: replay phase 4"
                );
                self.phase_4_commit(&run_id, &target_space_id, sponsor_space_person_id)
                    .await?;
                Ok(())
            }
        }
    }

    // ── 私有 phase 实现 ──────────────────────────────────────────────────

    async fn phase_1_prepare(&self) -> Result<MigrationRunId, SwitchSpaceError> {
        let active = active_space_placeholder();
        let run_id = self
            .key_migration
            .prepare_migration_key()
            .await
            .map_err(map_key_migration_err)?;
        debug!(run_id = %run_id, "migration key prepared");

        let reps = self
            .blob_migration_repo
            .list_main_inline_representations()
            .await
            .map_err(map_blob_repo_err)?;
        debug!(
            count = reps.len(),
            "found inline representations to back up"
        );

        for (event_id, rep_id) in reps {
            let bytes = match self
                .blob_migration_repo
                .read_main_inline_data(&event_id, &rep_id)
                .await
                .map_err(map_blob_repo_err)?
            {
                Some(b) => b,
                // 行被并发删了 / inline 已搬到 blob：跳过即可。
                None => continue,
            };
            let aad_bytes = aad::for_inline(&event_id, &rep_id);
            let aad_obj = Aad::from(aad_bytes);
            let plain = self
                .blob_cipher
                .decrypt(&active, &Ciphertext::new(bytes), &aad_obj)
                .await
                .map_err(map_blob_cipher_err)?;
            let mig_ct = self
                .key_migration
                .encrypt_with_migration_key(&run_id, &plain, &aad_obj)
                .await
                .map_err(map_key_migration_err)?;
            self.blob_migration_repo
                .upsert_record(&MigrationRecord {
                    event_id,
                    representation_id: rep_id,
                    migration_ciphertext: mig_ct.into_bytes(),
                })
                .await
                .map_err(map_blob_repo_err)?;
        }
        Ok(run_id)
    }

    async fn phase_2_handshake_and_persist(
        &self,
        code: &InvitationCode,
        new_passphrase: &Passphrase,
    ) -> Result<JoinerHandshakeOutcome, SwitchSpaceError> {
        let outcome = self
            .handshake
            .run(code, new_passphrase)
            .await
            .map_err(map_redeem_err)?;
        let now = self.now_utc()?;

        let admit_input = AdmitMember {
            device_id: outcome.sponsor_device_id.clone(),
            device_name: outcome.sponsor_device_name.clone(),
            identity_fingerprint: outcome.sponsor_identity_fingerprint.clone(),
            joined_at: now,
            sync_preferences: MemberSyncPreferences::default(),
        };
        self.admit_member
            .execute(admit_input)
            .await
            .map_err(map_admit_err)?;

        let trust_input = TrustPeer {
            local_device_id: outcome.self_device_id.clone(),
            peer_device_id: outcome.sponsor_device_id.clone(),
            peer_fingerprint: outcome.sponsor_identity_fingerprint.clone(),
            trusted_at: now,
        };
        self.trust_peer
            .execute(trust_input)
            .await
            .map_err(map_trust_err)?;

        // peer_addr_repo upsert 与 redeem use case 一致：失败仅 warn。
        if !outcome.sponsor_transport_address_blob.is_empty() {
            let record = PeerAddressRecord {
                device_id: outcome.sponsor_device_id.clone(),
                addr_blob: outcome.sponsor_transport_address_blob.clone(),
                observed_at: now,
            };
            if let Err(err) = self.peer_addr_repo.upsert(&record).await {
                warn!(error = %err, "peer_addr_repo.upsert failed (best-effort, ignored)");
            }
        }

        Ok(outcome)
    }

    async fn phase_3_swap(&self, run_id: &MigrationRunId) -> Result<(), SwitchSpaceError> {
        let active = active_space_placeholder();
        let records = self
            .blob_migration_repo
            .list_records()
            .await
            .map_err(map_blob_repo_err)?;
        debug!(count = records.len(), "phase 3: rewriting main table");

        for rec in records {
            let aad_bytes = aad::for_inline(&rec.event_id, &rec.representation_id);
            let aad_obj = Aad::from(aad_bytes);
            let mig_ct = Ciphertext::new(rec.migration_ciphertext);
            let plain = self
                .key_migration
                .decrypt_with_migration_key(run_id, &mig_ct, &aad_obj)
                .await
                .map_err(map_key_migration_err)?;
            let new_ct = self
                .blob_cipher
                .encrypt(&active, &plain, &aad_obj)
                .await
                .map_err(map_blob_cipher_err)?;
            self.blob_migration_repo
                .update_main_inline_data(&rec.event_id, &rec.representation_id, new_ct.as_bytes())
                .await
                .map_err(map_blob_repo_err)?;
        }
        Ok(())
    }

    async fn phase_4_commit(
        &self,
        run_id: &MigrationRunId,
        target_space_id: &SpaceId,
        identity_target: Option<Uuid>,
    ) -> Result<(), SwitchSpaceError> {
        // setup_status 切换到新 space_id（has_completed 已是 true）。
        self.setup_status
            .set_status(&SetupStatus {
                has_completed: true,
                space_id: Some(target_space_id.clone()),
            })
            .await
            .map_err(|e| SwitchSpaceError::Storage(e.to_string()))?;

        // Telemetry identity 切换：放在 setup_status 落盘之后、清掉
        // migration_state 之前。`Some` 走 sponsor 派发的 person，`None`
        // 回退到 Solo（v1→v2 未配对的 sponsor 场景）。adopt/release 内部
        // 是 fire-and-forget，失败只会 warn——只要这一步被调到，下次
        // capture 就会按新身份上报；即使本调用前进程崩了，重启后
        // resume_pending 会从持久化的 migration_state 恢复 identity_target
        // 并在 phase-4 replay 里再次调用，从而保证身份切换不会因为
        // commit→identify 之间的崩溃而被永久跳过。
        match identity_target {
            Some(target_person) => self.analytics.adopt_from_sponsor(target_person),
            None => self.analytics.release_to_solo(),
        }

        // Cleanup：失败仅 warn——下一次启动期补偿会再尝试清。
        if let Err(err) = self.blob_migration_repo.discard_all_records().await {
            warn!(
                error = %err,
                "phase 4: discard_all_records failed (will retry on next launch)"
            );
        }
        if let Err(err) = self.key_migration.discard_migration_key(run_id).await {
            warn!(
                error = %err,
                "phase 4: discard_migration_key failed (will be retried; harmless idle)"
            );
        }
        self.migration_state
            .set_current(None)
            .await
            .map_err(map_migration_state_err)?;
        Ok(())
    }

    /// Phase 1 中途失败时的 best-effort 清理。phase 1 失败前 migration_state
    /// 还没被推进到 `Prepared`，所以这里不需要清 `migration_state`。
    async fn cleanup_after_phase1_failure(&self) {
        if let Err(err) = self.blob_migration_repo.discard_all_records().await {
            warn!(error = %err, "phase1-cleanup: discard_all_records failed");
        }
    }

    /// Phase 2 失败 / `Prepared` 续跑放弃时的 best-effort 清理。
    async fn cleanup_after_phase2_failure(&self, run_id: &MigrationRunId) {
        if let Err(err) = self.blob_migration_repo.discard_all_records().await {
            warn!(error = %err, "phase2-cleanup: discard_all_records failed");
        }
        if let Err(err) = self.key_migration.discard_migration_key(run_id).await {
            warn!(error = %err, "phase2-cleanup: discard_migration_key failed");
        }
        if let Err(err) = self.migration_state.set_current(None).await {
            warn!(error = %err, "phase2-cleanup: migration_state.set_current(None) failed");
        }
    }

    fn now_utc(&self) -> Result<DateTime<Utc>, SwitchSpaceError> {
        DateTime::<Utc>::from_timestamp_millis(self.clock.now_ms())
            .ok_or_else(|| SwitchSpaceError::Internal("clock returned invalid timestamp".into()))
    }
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 单 master_key 模型下 `BlobCipherAdapter` 不按 `SpaceId` 路由——既有
/// `EncryptingClipboardEventWriter` / `DecryptingClipboardEventRepository`
/// 也是用占位 `ActiveSpace`。多空间分支后续 commit 再迁。
fn active_space_placeholder() -> ActiveSpace {
    ActiveSpace::new(SpaceId::from_str("space"))
}

fn map_blob_repo_err(err: BlobMigrationRepoError) -> SwitchSpaceError {
    match err {
        BlobMigrationRepoError::Storage(m) => SwitchSpaceError::Storage(m),
        BlobMigrationRepoError::Internal(m) => SwitchSpaceError::Internal(m),
    }
}

fn map_key_migration_err(err: KeyMigrationError) -> SwitchSpaceError {
    match err {
        KeyMigrationError::AlreadyExists(_) | KeyMigrationError::NotFound(_) => {
            SwitchSpaceError::Internal(err.to_string())
        }
        KeyMigrationError::InvalidCiphertext => SwitchSpaceError::InvalidCiphertext,
        KeyMigrationError::Internal(m) => SwitchSpaceError::Internal(m),
    }
}

fn map_blob_cipher_err(err: BlobCipherError) -> SwitchSpaceError {
    match err {
        BlobCipherError::NotUnlocked => SwitchSpaceError::NotUnlocked,
        BlobCipherError::InvalidCiphertext => SwitchSpaceError::InvalidCiphertext,
        BlobCipherError::Internal(m) => SwitchSpaceError::Internal(m),
    }
}

fn map_migration_state_err(err: MigrationStateError) -> SwitchSpaceError {
    match err {
        MigrationStateError::Storage(m) => SwitchSpaceError::Storage(m),
        MigrationStateError::Internal(m) => SwitchSpaceError::Internal(m),
    }
}

fn map_admit_err(err: MembershipApplicationError) -> SwitchSpaceError {
    SwitchSpaceError::Internal(format!("admit_member: {err}"))
}

fn map_trust_err(err: TrustedPeerApplicationError) -> SwitchSpaceError {
    SwitchSpaceError::Internal(format!("trust_peer: {err}"))
}

fn map_redeem_err(err: RedeemPairingInvitationError) -> SwitchSpaceError {
    use RedeemPairingInvitationError as R;
    match err {
        R::InvitationNotFound => SwitchSpaceError::InvitationNotFound,
        R::InvitationExpired => SwitchSpaceError::InvitationExpired,
        R::SponsorUnreachable => SwitchSpaceError::SponsorUnreachable,
        R::ServiceUnavailable => SwitchSpaceError::ServiceUnavailable,
        R::PassphraseMismatch => SwitchSpaceError::PassphraseMismatch,
        R::CorruptedKeyMaterial => SwitchSpaceError::CorruptedKeyMaterial,
        R::DeviceNameRequired => SwitchSpaceError::DeviceNameRequired,
        R::SponsorRejectedInvitation => SwitchSpaceError::SponsorRejectedInvitation,
        R::SponsorDeclined => SwitchSpaceError::SponsorDeclined,
        R::SponsorTimedOut => SwitchSpaceError::Timeout,
        R::Timeout => SwitchSpaceError::Timeout,
        R::ConnectionLost => SwitchSpaceError::ConnectionLost,
        R::SponsorInternal(m) => SwitchSpaceError::Internal(format!("sponsor: {m}")),
        R::Internal(m) => SwitchSpaceError::Internal(m),
    }
}

#[cfg(test)]
mod tests;
