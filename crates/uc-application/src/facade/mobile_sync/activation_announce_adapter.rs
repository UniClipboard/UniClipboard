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
//! # 错误降级
//!
//! `announce_local_activation` 内部已对 register / dispatch 失败做 best-effort
//! 降级; 这里只需对 duplicate 的 OS 写回失败 `warn!`, 不抛回上层 use case
//! —— mobile 上传是否成功只取决于本机入站管线的 outcome, 收敛是事后传播。
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

pub(crate) struct MobileActivationAnnounceAdapter {
    coordinator: Arc<ClipboardWriteCoordinator>,
    active_clipboard: Arc<ActiveClipboardFacade>,
}

impl MobileActivationAnnounceAdapter {
    pub(crate) fn new(
        coordinator: Arc<ClipboardWriteCoordinator>,
        active_clipboard: Arc<ActiveClipboardFacade>,
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
        let content_hash = snapshot.snapshot_hash().to_string();
        let categories = ClipboardContentCategorySet::from_snapshot(snapshot);
        self.active_clipboard
            .announce_local_activation(content_hash, entry_id, categories)
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
        if let Err(err) = self
            .coordinator
            .write(snapshot.clone(), ClipboardWriteIntent::LocalRestore)
            .await
        {
            warn!(
                entry_id = %entry_id,
                error = %err,
                "mobile_sync duplicate announce: failed to re-write existing content to system clipboard"
            );
        }
        self.converge(entry_id, &snapshot).await;
    }
}
