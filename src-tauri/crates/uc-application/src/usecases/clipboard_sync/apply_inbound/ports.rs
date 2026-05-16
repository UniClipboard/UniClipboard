//! ApplyInbound 的两个内部端口抽象 —— 持久化与 OS 剪贴板写入。
//!
//! 用 `Arc<dyn Trait>` 而不是直接持有 `CaptureClipboardUseCase` /
//! `ClipboardWriteCoordinator` 是为了让 use case 在测试里能 mock,而不必构造
//! 完整的 7+2 port 依赖图。生产环境通过下面两个 blanket impl 装配真实类型。

use anyhow::Result;
use async_trait::async_trait;
use uc_core::ids::EntryId;
use uc_core::{ClipboardChangeOrigin, DeviceId, SystemClipboardSnapshot};

use crate::clipboard_capture::CaptureClipboardUseCase;
use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};

/// Internal abstraction over the persistence pipeline. Production uses
/// the blanket impl on `CaptureClipboardUseCase`; tests use a `mockall`
/// mock.
#[async_trait]
pub trait InboundCapture: Send + Sync {
    /// Persist `snapshot` as a `RemotePush`-origin entry under the
    /// caller-supplied `preset_entry_id`. The caller (ApplyInbound) decides
    /// the entry_id at the very start of the inbound pipeline so that
    /// blob-fetch progress events and the eventual `clipboard.new_content`
    /// event share the same id; the frontend can then key its placeholder
    /// card on this id and let it be replaced by the real entry without a
    /// transfer_id → entry_id remap step.
    ///
    /// `from_device` 是推送方 device id,落库时会写入 `ClipboardEvent.source_device`
    /// 让上层视图(delivery view)正确识别来源为远端而非本机。
    ///
    /// Returns `Ok(Some(entry_id))` on success, `Ok(None)` only in the
    /// legitimate "no supported representation" / `LocalRestore`
    /// short-circuit cases (which `RemotePush` never hits in practice —
    /// daemon treats `None` as `ApplyInboundError::Internal`).
    async fn capture(
        &self,
        preset_entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<Option<EntryId>>;
}

#[async_trait]
impl InboundCapture for CaptureClipboardUseCase {
    async fn capture(
        &self,
        preset_entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<Option<EntryId>> {
        self.execute_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            Some(preset_entry_id),
        )
        .await
    }
}

/// Internal abstraction over the OS clipboard write boundary. Production
/// uses the blanket impl on `ClipboardWriteCoordinator`; tests mock it.
#[async_trait]
pub trait InboundWrite: Send + Sync {
    /// Write `snapshot` to the OS clipboard with the `RemotePush`
    /// intent (registers the appropriate hash guards + next-origin
    /// override per the coordinator's contract).
    async fn write(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
}

#[async_trait]
impl InboundWrite for ClipboardWriteCoordinator {
    async fn write(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        ClipboardWriteCoordinator::write(self, snapshot, ClipboardWriteIntent::RemotePush).await
    }
}
