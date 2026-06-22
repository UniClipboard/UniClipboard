//! `MobileActivationAnnounceAdapter` —— [`MobileActivationAnnouncePort`] 的
//! 生产实现, 把移动端入站激活接到跨设备 active-clipboard 收敛 (issue #1017
//! D1 call-sites 3 & 4, D2 "Mobile push → fan-out")。
//!
//! # 设计意图
//!
//! `ApplyIncomingMobileClipUseCase` 通过 [`MobileActivationAnnouncePort`]
//! 这层薄抽象与"如何收敛一次本设备激活"解耦 ——
//!
//! - **测试时**: fake 实现直接 record 调用, 不必拉真实 coordinator /
//!   register / dispatch;
//! - **生产时**: 本 adapter 承担两件事:
//!   1. duplicate 命中时, 用这次上传的 snapshot 把内容写回系统剪贴板
//!      (`ClipboardWriteCoordinator`, `LocalRestore` intent —— 同本机
//!      restore 一样的写回环防御);new 内容由入站管线写过, 跳过这步;
//!   2. 不论新旧, 都委托 [`ActiveClipboardFacade::announce_local_activation`]
//!      盖本设备激活戳 (`activated_by = self`, `activated_at_ms = now`)、
//!      前进跨设备 register、按 per-device send 闸门 (`send_enabled` ∧
//!      `send_content_types`) 广播 0xC3 state。
//!
//! # 闸门
//!
//! 收敛只受 per-device send 闸门约束, **不**看 `sync_on_restore` —— 移动端
//! 推送是本设备的一次主动激活, 与历史 restore 广播是两条独立路径。
//!
//! # OS-write coupling (issue #1017 §1 invariant)
//!
//! `announce_local_activation` internally degrades best-effort on
//! register / dispatch failure. The OS re-write on the `announce_duplicate`
//! path, however, is part of the core invariant
//! (register-advance <=> OS-write-success <=> re-broadcast): if it fails
//! (e.g. the write coordinator's circuit breaker is open), the converge is
//! skipped so this device cannot pin peers to a phantom activation it does not
//! actually hold. The failure is `warn!`-logged but never propagated to the
//! use case — the mobile upload's success depends only on the inbound
//! pipeline, and convergence is after-the-fact propagation. `announce_new`
//! does not re-write (the inbound pipeline already wrote the OS clipboard), so
//! it converges unconditionally.
//!
//! [`MobileActivationAnnouncePort`]: crate::usecases::mobile_sync::apply_incoming::MobileActivationAnnouncePort
//! [`ActiveClipboardFacade`]: crate::facade::active_clipboard::ActiveClipboardFacade

use std::sync::Arc;

use tracing::warn;

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::EntryId;
use uc_core::SystemClipboardSnapshot;

use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};
use crate::facade::active_clipboard::ActiveClipboardFacade;
use crate::usecases::mobile_sync::apply_incoming::MobileActivationAnnouncePort;

/// Narrow seam over [`ActiveClipboardFacade::announce_local_activation`]: stamp
/// a local activation, advance the cross-device register, and fan the 0xC3
/// state out under the per-device send gate.
///
/// Existing only so the adapter's OS-write gating (`announce_duplicate`) can be
/// unit-tested without standing up the full active-clipboard facade (~25
/// ports). Production binds this to the real facade; tests bind a spy that
/// records whether convergence ran.
#[async_trait::async_trait]
pub(crate) trait LocalActivationConverge: Send + Sync {
    async fn announce_local_activation(
        &self,
        snapshot_hash: String,
        entry_id: EntryId,
        categories: ClipboardContentCategorySet,
    );
}

#[async_trait::async_trait]
impl LocalActivationConverge for ActiveClipboardFacade {
    async fn announce_local_activation(
        &self,
        snapshot_hash: String,
        entry_id: EntryId,
        categories: ClipboardContentCategorySet,
    ) {
        // Fully-qualified call resolves to the inherent method (inherent
        // methods take precedence over trait methods), not this trait impl.
        ActiveClipboardFacade::announce_local_activation(self, snapshot_hash, entry_id, categories)
            .await;
    }
}

pub(crate) struct MobileActivationAnnounceAdapter {
    coordinator: Arc<ClipboardWriteCoordinator>,
    active_clipboard: Arc<dyn LocalActivationConverge>,
}

impl MobileActivationAnnounceAdapter {
    pub(crate) fn new(
        coordinator: Arc<ClipboardWriteCoordinator>,
        active_clipboard: Arc<dyn LocalActivationConverge>,
    ) -> Self {
        Self {
            coordinator,
            active_clipboard,
        }
    }

    /// Derive the cross-device activation key + content category set from the
    /// snapshot, then advance the register and fan the 0xC3 state out under the
    /// per-device send gate. Shared tail of both `announce_*` paths.
    async fn converge(&self, entry_id: EntryId, snapshot: &SystemClipboardSnapshot) {
        let snapshot_hash = snapshot.snapshot_hash().to_string();
        let categories = ClipboardContentCategorySet::from_snapshot(snapshot);
        self.active_clipboard
            .announce_local_activation(snapshot_hash, entry_id, categories)
            .await;
    }
}

#[async_trait::async_trait]
impl MobileActivationAnnouncePort for MobileActivationAnnounceAdapter {
    async fn announce_new(&self, entry_id: EntryId, snapshot: SystemClipboardSnapshot) {
        // Inbound apply already wrote the OS clipboard; only converge peers.
        self.converge(entry_id, &snapshot).await;
    }

    async fn announce_duplicate(&self, entry_id: EntryId, snapshot: SystemClipboardSnapshot) {
        // Content already held locally, but the OS clipboard may have been
        // overwritten by later copies. Re-write this upload's snapshot so the
        // user's next paste yields it, then converge peers like a new push.
        //
        // Invariant (issue #1017 §1): register-advance <=> OS-write-success <=>
        // re-broadcast. If the re-write fails (e.g. the coordinator's circuit
        // breaker is open), skip the converge — otherwise this device would
        // stamp a high LWW ts and broadcast a 0xC3 state for content its OS
        // clipboard does not actually hold, pinning peers to a phantom
        // activation. The mobile upload itself already succeeded; convergence
        // is best-effort after-the-fact propagation, so dropping it here is
        // safe.
        if let Err(err) = self
            .coordinator
            .write(snapshot.clone(), ClipboardWriteIntent::LocalRestore)
            .await
        {
            warn!(
                entry_id = %entry_id,
                error = %err,
                "mobile_sync duplicate announce: OS clipboard re-write failed; \
                 skipping register advance + 0xC3 fan-out"
            );
            return;
        }
        self.converge(entry_id, &snapshot).await;
    }
}

#[cfg(test)]
mod tests {
    //! `announce_duplicate` must honour the issue #1017 §1 invariant: a failed
    //! OS re-write must NOT advance the register / broadcast 0xC3. The seam
    //! trait lets us assert that with a trivially-failing write coordinator.
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;

    use uc_core::clipboard::ClipboardChangeOrigin;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::ports::clipboard::{ClipboardChangeOriginPort, SystemClipboardPort};
    use uc_core::{MimeType, ObservedClipboardRepresentation};

    /// Records how many times convergence (register advance + 0xC3 fan-out) ran.
    #[derive(Default)]
    struct SpyConverge {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl LocalActivationConverge for SpyConverge {
        async fn announce_local_activation(
            &self,
            _snapshot_hash: String,
            _entry_id: EntryId,
            _categories: ClipboardContentCategorySet,
        ) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// System clipboard whose write outcome is fixed at construction.
    struct FixedWriter {
        write_ok: bool,
    }

    impl SystemClipboardPort for FixedWriter {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            unreachable!("the announce adapter never reads the OS clipboard")
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            if self.write_ok {
                Ok(())
            } else {
                Err(anyhow::anyhow!("simulated OS clipboard write failure"))
            }
        }
    }

    /// Origin guard port with no behaviour — the coordinator drives it but its
    /// calls are irrelevant to this test.
    struct NoopOrigin;

    #[async_trait]
    impl ClipboardChangeOriginPort for NoopOrigin {
        async fn set_next_origin(&self, _origin: ClipboardChangeOrigin, _ttl: Duration) {}

        async fn consume_origin_or_default(
            &self,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            default_origin
        }
    }

    fn text_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"hi".to_vec(),
            )],
            file_content_digests: Vec::new(),
        }
    }

    fn adapter_with(write_ok: bool, spy: Arc<SpyConverge>) -> MobileActivationAnnounceAdapter {
        let coordinator = Arc::new(ClipboardWriteCoordinator::new(
            Arc::new(FixedWriter { write_ok }),
            Arc::new(NoopOrigin),
        ));
        MobileActivationAnnounceAdapter::new(coordinator, spy)
    }

    #[tokio::test]
    async fn duplicate_skips_converge_when_os_write_fails() {
        let spy = Arc::new(SpyConverge::default());
        let adapter = adapter_with(false, spy.clone());

        adapter
            .announce_duplicate(EntryId::new(), text_snapshot())
            .await;

        assert_eq!(
            spy.calls.load(Ordering::SeqCst),
            0,
            "failed OS re-write must not advance the register or broadcast 0xC3"
        );
    }

    #[tokio::test]
    async fn duplicate_converges_once_when_os_write_succeeds() {
        let spy = Arc::new(SpyConverge::default());
        let adapter = adapter_with(true, spy.clone());

        adapter
            .announce_duplicate(EntryId::new(), text_snapshot())
            .await;

        assert_eq!(
            spy.calls.load(Ordering::SeqCst),
            1,
            "successful OS re-write must converge exactly once"
        );
    }

    #[tokio::test]
    async fn new_converges_unconditionally_without_os_write() {
        // announce_new does not re-write the OS clipboard (the inbound pipeline
        // already did), so a failing writer must not affect it.
        let spy = Arc::new(SpyConverge::default());
        let adapter = adapter_with(false, spy.clone());

        adapter.announce_new(EntryId::new(), text_snapshot()).await;

        assert_eq!(
            spy.calls.load(Ordering::SeqCst),
            1,
            "announce_new converges once, independent of any OS write"
        );
    }
}
