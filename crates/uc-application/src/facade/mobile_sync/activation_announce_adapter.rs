//! `MobileActivationAnnounceAdapter` —— [`MobileActivationAnnouncePort`] 的
//! 生产实现, 把移动端入站激活接到跨设备 active-clipboard 收敛 (issue #1017
//! D1 call-sites 3 & 4, D2 "Mobile push → fan-out")。
//!
//! # 设计意图
//!
//! `ApplyIncomingMobileClipUseCase` 通过 [`MobileActivationAnnouncePort`]
//! 这层薄抽象与"如何收敛一次本设备激活"解耦 ——
//!
//! - **测试时**: fake 实现直接 record 调用, 不必拉真实 register / dispatch;
//! - **生产时**: 本 adapter 委托
//!   [`ActiveClipboardFacade::announce_local_activation`] 盖本设备激活戳
//!   (`activated_by = self`, `activated_at_ms = now`)、前进跨设备 register、
//!   按 per-device send 闸门 (`send_enabled` ∧ `send_content_types`) 广播
//!   0xC3 state。
//!
//! # 闸门
//!
//! 收敛只受 per-device send 闸门约束, **不**看 `sync_on_restore` —— 移动端
//! 推送是本设备的一次主动激活, 与历史 restore 广播是两条独立路径。
//!
//! # OS-write coupling (issue #1017 §1 invariant)
//!
//! 不变式 (register-advance <=> OS-write-success <=> re-broadcast) 依然成立,
//! 但本 adapter 不再是它的执行点: 系统剪贴板由入站管线统一写(新内容走落库
//! 后的后台写, 已有内容走 dedup 命中的重激活写), 调用方
//! `ApplyIncomingMobileClipUseCase::maybe_announce_activation` 只在那次写
//! 确认落地后才调到这里。因此本 adapter 无条件收敛, 不持写边界 ——
//! 写失败的情况在调用方就被挡掉了。
//!
//! `announce_local_activation` 内部对 register / dispatch 失败仍是
//! best-effort 降级。
//!
//! [`MobileActivationAnnouncePort`]: crate::usecases::mobile_sync::apply_incoming::MobileActivationAnnouncePort
//! [`ActiveClipboardFacade`]: crate::facade::active_clipboard::ActiveClipboardFacade

use std::sync::Arc;

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::EntryId;
use uc_core::SystemClipboardSnapshot;

use crate::facade::active_clipboard::ActiveClipboardFacade;
use crate::usecases::mobile_sync::apply_incoming::MobileActivationAnnouncePort;

/// Narrow seam over [`ActiveClipboardFacade::announce_local_activation`]: stamp
/// a local activation, advance the cross-device register, and fan the 0xC3
/// state out under the per-device send gate.
///
/// Existing so the adapter can be unit-tested without standing up the full
/// active-clipboard facade (~25 ports). Production binds this to the real
/// facade; tests bind a spy that records whether convergence ran.
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
    active_clipboard: Arc<dyn LocalActivationConverge>,
}

impl MobileActivationAnnounceAdapter {
    pub(crate) fn new(active_clipboard: Arc<dyn LocalActivationConverge>) -> Self {
        Self { active_clipboard }
    }
}

#[async_trait::async_trait]
impl MobileActivationAnnouncePort for MobileActivationAnnounceAdapter {
    async fn announce_new(&self, entry_id: EntryId, snapshot: SystemClipboardSnapshot) {
        // The inbound pipeline owns the OS write for both new and re-activated
        // content, and the use case only calls this once that write is known to
        // have landed — so converging here upholds the issue #1017 §1 invariant
        // (register-advance <=> OS-write-success <=> re-broadcast) without
        // needing to touch the clipboard itself.
        let snapshot_hash = snapshot.snapshot_hash().to_string();
        let categories = ClipboardContentCategorySet::from_snapshot(&snapshot);
        self.active_clipboard
            .announce_local_activation(snapshot_hash, entry_id, categories)
            .await;
    }
}

#[cfg(test)]
mod tests {
    //! The adapter is now a thin converge seam: the OS write lives in the
    //! inbound pipeline, and the issue #1017 §1 gate (skip convergence when
    //! that write failed) is enforced by the caller in
    //! `ApplyIncomingMobileClipUseCase::maybe_announce_activation`.
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::{MimeType, ObservedClipboardRepresentation};

    /// Records how many times convergence (register advance + 0xC3 fan-out) ran.
    #[derive(Default)]
    struct SpyConverge {
        calls: AtomicUsize,
        last_hash: std::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl LocalActivationConverge for SpyConverge {
        async fn announce_local_activation(
            &self,
            snapshot_hash: String,
            _entry_id: EntryId,
            _categories: ClipboardContentCategorySet,
        ) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_hash.lock().unwrap() = Some(snapshot_hash);
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
            file_set_v1_component: None,
        }
    }

    #[tokio::test]
    async fn announce_converges_once_under_the_snapshot_identity() {
        let spy = Arc::new(SpyConverge::default());
        let adapter = MobileActivationAnnounceAdapter::new(spy.clone());
        let snapshot = text_snapshot();
        let expected_hash = snapshot.snapshot_hash().to_string();

        adapter.announce_new(EntryId::new(), snapshot).await;

        assert_eq!(spy.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            spy.last_hash.lock().unwrap().as_deref(),
            Some(expected_hash.as_str()),
            "convergence must key off the snapshot's own identity"
        );
    }
}
