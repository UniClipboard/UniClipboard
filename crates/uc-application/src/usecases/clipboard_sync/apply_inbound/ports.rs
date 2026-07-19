//! ApplyInbound 的两个内部端口抽象 —— 持久化与 OS 剪贴板写入。
//!
//! 用 `Arc<dyn Trait>` 而不是直接持有 `CaptureClipboardUseCase` /
//! `ClipboardWriteCoordinator` 是为了让 use case 在测试里能 mock,而不必构造
//! 完整的 7+2 port 依赖图。生产环境通过下面两个 blanket impl 装配真实类型。

use anyhow::Result;
use async_trait::async_trait;
use uc_core::ids::EntryId;
use uc_core::{ClipboardChangeOrigin, DeviceId, SnapshotHash, SystemClipboardSnapshot};

use crate::clipboard_capture::{
    CaptureClipboardUseCase, CommitMode, DirectoryCaptureCommitContext, InboundCaptureCommitContext,
};
use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};
use crate::usecases::clipboard_sync::snapshot_from_entry::SnapshotReconstructor;

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

    /// Like [`Self::capture`] but persists the entry under `authoritative_hash`
    /// — the cross-device identity the sender advertised on the wire — instead
    /// of a hash recomputed from the materialized snapshot.
    ///
    /// This is required so a partial (cancelled-transfer) entry shares its
    /// identity with the eventual completed delivery and with every other
    /// channel carrying the same wire hash, rather than forking into a separate
    /// entry. `None` degrades to recomputing (e.g. an unparseable wire hash),
    /// preserving prior behavior without panicking.
    ///
    /// The default delegates to [`Self::capture`] (ignoring the identity); only
    /// the production persistence pipeline honors it. Inbound callers use this
    /// method, never [`Self::capture`] directly.
    async fn capture_with_identity(
        &self,
        preset_entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
    ) -> Result<Option<EntryId>> {
        let _ = authoritative_hash;
        self.capture(preset_entry_id, from_device, snapshot).await
    }

    /// Replace the content behind the existing entry `entry_id` in place with
    /// `snapshot`, under `authoritative_hash` (the sender's wire identity). Used
    /// by the inbound upgrade path when a completed delivery supersedes a
    /// partial entry that already carries this content hash: the entry keeps its
    /// id and sticky state while its content is swapped transactionally.
    ///
    /// The default delegates to [`Self::capture_with_identity`] (a plain create
    /// under the same id), so mocks that only implement [`Self::capture`] keep
    /// working; only the production persistence pipeline performs a true
    /// in-place replace.
    async fn replace_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
    ) -> Result<Option<EntryId>> {
        self.capture_with_identity(entry_id, from_device, snapshot, authoritative_hash)
            .await
    }

    async fn capture_inbound_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
        _commit: InboundCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        self.capture_with_identity(entry_id, from_device, snapshot, authoritative_hash)
            .await
    }

    async fn replace_inbound_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
        _commit: InboundCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        self.replace_with_identity(entry_id, from_device, snapshot, authoritative_hash)
            .await
    }

    async fn capture_directory_with_identity(
        &self,
        _entry_id: EntryId,
        _from_device: DeviceId,
        _snapshot: SystemClipboardSnapshot,
        _authoritative_hash: Option<SnapshotHash>,
        _commit: DirectoryCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        anyhow::bail!("directory receive commit is not supported by this capture adapter")
    }

    async fn replace_directory_with_identity(
        &self,
        _entry_id: EntryId,
        _from_device: DeviceId,
        _snapshot: SystemClipboardSnapshot,
        _authoritative_hash: Option<SnapshotHash>,
        _commit: DirectoryCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        anyhow::bail!("directory receive commit is not supported by this capture adapter")
    }
}

#[async_trait]
impl InboundCapture for CaptureClipboardUseCase {
    async fn capture(
        &self,
        preset_entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<Option<EntryId>> {
        self.capture_with_identity(preset_entry_id, from_device, snapshot, None)
            .await
    }

    async fn capture_with_identity(
        &self,
        preset_entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
    ) -> Result<Option<EntryId>> {
        self.execute_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            Some(preset_entry_id),
            authoritative_hash,
            CommitMode::Create,
        )
        .await
        // RemotePush never takes the local dedup branch, so the outcome is
        // always a fresh entry; the inbound contract only needs its id.
        .map(|outcome| outcome.map(|o| o.entry_id))
    }

    async fn replace_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
    ) -> Result<Option<EntryId>> {
        self.execute_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            Some(entry_id),
            authoritative_hash,
            CommitMode::Replace,
        )
        .await
        .map(|outcome| outcome.map(|o| o.entry_id))
    }

    async fn capture_inbound_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
        commit: InboundCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        self.execute_inbound_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            entry_id,
            authoritative_hash,
            CommitMode::Create,
            commit,
        )
        .await
        .map(|outcome| outcome.map(|o| o.entry_id))
    }

    async fn replace_inbound_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
        commit: InboundCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        self.execute_inbound_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            entry_id,
            authoritative_hash,
            CommitMode::Replace,
            commit,
        )
        .await
        .map(|outcome| outcome.map(|o| o.entry_id))
    }

    async fn capture_directory_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
        commit: DirectoryCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        self.execute_directory_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            entry_id,
            authoritative_hash,
            CommitMode::Create,
            commit,
        )
        .await
        .map(|outcome| outcome.map(|o| o.entry_id))
    }

    async fn replace_directory_with_identity(
        &self,
        entry_id: EntryId,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        authoritative_hash: Option<SnapshotHash>,
        commit: DirectoryCaptureCommitContext,
    ) -> Result<Option<EntryId>> {
        self.execute_directory_with_origin(
            snapshot,
            ClipboardChangeOrigin::RemotePush {
                from_device: Some(from_device),
            },
            entry_id,
            authoritative_hash,
            CommitMode::Replace,
            commit,
        )
        .await
        .map(|outcome| outcome.map(|o| o.entry_id))
    }
}

/// Internal abstraction over "rebuild the snapshot this entry holds locally".
/// Production uses the blanket impl on `SnapshotReconstructor` (6 ports); tests
/// mock this one method.
#[async_trait]
pub trait InboundSnapshotRebuild: Send + Sync {
    /// Rebuild `entry_id`'s snapshot from local storage, resolving every
    /// representation's bytes. Used to re-activate held content without
    /// re-downloading the sender's payload.
    async fn rebuild(&self, entry_id: &EntryId) -> Result<SystemClipboardSnapshot>;
}

#[async_trait]
impl InboundSnapshotRebuild for SnapshotReconstructor {
    async fn rebuild(&self, entry_id: &EntryId) -> Result<SystemClipboardSnapshot> {
        self.reconstruct(entry_id).await.map_err(Into::into)
    }
}

/// Internal abstraction over the OS clipboard write boundary. Production
/// uses the blanket impl on `ClipboardWriteCoordinator`; tests mock it.
#[async_trait]
pub trait InboundWrite: Send + Sync {
    /// Write `snapshot` to the OS clipboard under `intent` (registers the
    /// appropriate hash guards + next-origin override per the coordinator's
    /// contract).
    ///
    /// Fresh content always arrives as `RemotePush`; the resurface path passes
    /// [`ApplyInboundInput::resurface_intent`], which differs per inbound
    /// channel — see its docs for why.
    ///
    /// [`ApplyInboundInput::resurface_intent`]: super::ApplyInboundInput::resurface_intent
    async fn write(
        &self,
        snapshot: SystemClipboardSnapshot,
        intent: ClipboardWriteIntent,
    ) -> Result<()>;
}

#[async_trait]
impl InboundWrite for ClipboardWriteCoordinator {
    async fn write(
        &self,
        snapshot: SystemClipboardSnapshot,
        intent: ClipboardWriteIntent,
    ) -> Result<()> {
        ClipboardWriteCoordinator::write(self, snapshot, intent).await
    }
}
