//! `ClipboardRestoreOnDuplicateAdapter` —— [`MobileDuplicateRestorePort`]
//! 的生产实现, 把 mobile 入站去重命中事件接到 [`ClipboardRestoreFacade`]
//! 的 restore_entry 管线。
//!
//! # 设计意图
//!
//! `ApplyIncomingMobileClipUseCase` 通过 [`MobileDuplicateRestorePort`]
//! 这层薄抽象与"如何从 entry_id 恢复到 OS 剪贴板"解耦 ——
//!
//! - **测试时**: fake 实现直接 record 调用, 不必拉真实 DB / OS clipboard;
//! - **生产时**: 本 adapter 委托 `ClipboardRestoreFacade::restore_entry`,
//!   走完整的 snapshot 重建 → `ClipboardWriteCoordinator` 写 OS 剪贴板。
//!
//! # 错误降级
//!
//! `restore_entry` 失败仅 `warn!`, 不抛回上层 use case —— mobile 上传
//! 是否成功只取决于"本机入站管线的 outcome", restore 是让用户能立刻
//! Cmd-V 的 best-effort 增强。
//!
//! [`MobileDuplicateRestorePort`]: crate::usecases::mobile_sync::apply_incoming::MobileDuplicateRestorePort
//! [`ClipboardRestoreFacade`]: crate::facade::clipboard_restore::ClipboardRestoreFacade

use std::sync::Arc;

use tracing::warn;

use uc_core::ids::EntryId;

use crate::facade::clipboard_restore::ClipboardRestoreFacade;
use crate::usecases::mobile_sync::apply_incoming::MobileDuplicateRestorePort;

pub(crate) struct ClipboardRestoreOnDuplicateAdapter {
    restore_facade: Arc<ClipboardRestoreFacade>,
}

impl ClipboardRestoreOnDuplicateAdapter {
    pub(crate) fn new(restore_facade: Arc<ClipboardRestoreFacade>) -> Self {
        Self { restore_facade }
    }
}

#[async_trait::async_trait]
impl MobileDuplicateRestorePort for ClipboardRestoreOnDuplicateAdapter {
    async fn restore_to_clipboard(&self, entry_id: &EntryId) {
        if let Err(err) = self.restore_facade.restore_entry(entry_id.as_ref()).await {
            warn!(
                entry_id = %entry_id,
                error = %err,
                "mobile_sync duplicate restore: failed to restore existing entry to system clipboard"
            );
        }
    }
}
