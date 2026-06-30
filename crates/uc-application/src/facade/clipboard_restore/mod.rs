use std::sync::Arc;

use tracing::instrument;
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ids::EntryId;
use uc_core::ports::{
    clipboard::{AdvanceActiveClipboardPort, ClipboardPayloadResolverPort, PayloadResolveError},
    ClipboardSelectionRepositoryPort, ClockPort, DeviceIdentityPort,
};

use crate::deps::{ClipboardEntryPorts, ClipboardRepresentationPorts};

use crate::clipboard_write::{
    ClipboardWriteCoordinator, LocalActiveRegisterAdvancer, RestoreBroadcastTrigger,
};
use crate::usecases::clipboard_restore::{
    PlainRestoreOutcome, RestoreClipboardEntryAsPlainTextUseCase, RestoreClipboardSelectionUseCase,
    TouchClipboardEntryUseCase,
};

#[derive(Debug, thiserror::Error)]
pub enum ClipboardRestoreError {
    #[error("clipboard entry not found")]
    NotFound,

    /// Paste representation can no longer be materialized — bytes are gone
    /// from cache and spool, or the representation is in `Lost` state.
    /// This is a known business outcome (resource has logically vanished),
    /// not a server fault. API layer should map this to 410 Gone, **not** 500.
    #[error(
        "clipboard payload unavailable: representation {rep_id} for entry {entry_id} (state={state})"
    )]
    PayloadUnavailable {
        entry_id: String,
        rep_id: String,
        state: String,
    },

    #[error("clipboard restore failed: {0}")]
    Internal(String),
}

/// Dependency bundle for `ClipboardRestoreFacade`. Composition roots build
/// this once from their wiring deps and pass it to
/// `ClipboardRestoreFacade::new`.
pub struct ClipboardRestoreFacadeDeps {
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    /// Narrow entry intent ports: `get` feeds both the multi-format restore
    /// path (via the shared `reconstruct_snapshot_from_entry` helper) and the
    /// plain-text restore path; `touch` feeds the post-restore LRU touch.
    pub entry_ports: ClipboardEntryPorts,
    /// Narrow representation intent ports: `get` feeds both restore paths'
    /// candidate lookup; `update_processing_result` feeds the shared helper's
    /// orphan→Lost demotion side effect.
    pub representation_ports: ClipboardRepresentationPorts,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub clock: Arc<dyn ClockPort>,
    /// Identity of this device, used to stamp the active-clipboard register
    /// when a restore makes its content the latest active clipboard state.
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    /// Cross-device active-clipboard register advanced after a successful
    /// restore.
    pub active_register: Arc<dyn AdvanceActiveClipboardPort>,
    /// Optional restore-broadcast trigger. When present, a successful restore
    /// that advanced the register also offers the activation to the broadcast
    /// subsystem (which gates on `sync_on_restore` + per-device send prefs
    /// before announcing to peers). `None` where no broadcaster is wired
    /// (e.g. CLI fallback without a daemon network stack).
    pub restore_broadcast: Option<RestoreBroadcastTrigger>,
    pub write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub integration_mode: ClipboardIntegrationMode,
}

pub struct ClipboardRestoreFacade {
    restore_uc: RestoreClipboardSelectionUseCase,
    plain_uc: RestoreClipboardEntryAsPlainTextUseCase,
    touch_uc: TouchClipboardEntryUseCase,
}

impl ClipboardRestoreFacade {
    pub fn new(deps: ClipboardRestoreFacadeDeps) -> Self {
        let ClipboardRestoreFacadeDeps {
            selection_repo,
            entry_ports,
            representation_ports,
            payload_resolver,
            blob_store,
            clock,
            device_identity,
            active_register,
            restore_broadcast,
            write_coordinator,
            integration_mode,
        } = deps;

        // Shared advancer wired into both restore paths so any successful
        // restore advances the cross-device active-clipboard register.
        let register_advancer =
            LocalActiveRegisterAdvancer::new(active_register, device_identity, clock.clone());

        let ClipboardEntryPorts {
            get: entry_get,
            list: _entry_list,
            save: _entry_save,
            touch: entry_touch,
            set_favorite: _entry_set_favorite,
            delete: _entry_delete,
            find_by_snapshot_hash: _entry_find,
            get_snapshot_hash: entry_snapshot_hash_lookup,
            availability: _entry_availability,
            replace_content: _entry_replace,
        } = entry_ports;
        let ClipboardRepresentationPorts {
            get: rep_get,
            get_by_blob_id: _rep_get_by_blob_id,
            list_for_event: _rep_list_for_event,
            update_processing_result: rep_update,
        } = representation_ports;

        let mut restore_uc = RestoreClipboardSelectionUseCase::new(
            entry_get.clone(),
            write_coordinator.clone(),
            selection_repo.clone(),
            rep_get.clone(),
            rep_update,
            payload_resolver.clone(),
            blob_store.clone(),
            integration_mode,
        )
        .with_active_register(
            register_advancer.clone(),
            entry_snapshot_hash_lookup.clone(),
        );
        let mut plain_uc = RestoreClipboardEntryAsPlainTextUseCase::new(
            entry_get,
            write_coordinator,
            selection_repo,
            rep_get,
            payload_resolver,
            blob_store,
            integration_mode,
        )
        .with_active_register(register_advancer, entry_snapshot_hash_lookup);
        // Wire the restore-broadcast trigger into both paths when present, so a
        // successful restore that advanced the register also announces it
        // (subject to the broadcaster's gate). Shared trigger; cloning is cheap.
        if let Some(trigger) = restore_broadcast {
            restore_uc = restore_uc.with_restore_broadcast(trigger.clone());
            plain_uc = plain_uc.with_restore_broadcast(trigger);
        }
        let touch_uc = TouchClipboardEntryUseCase::new(entry_touch, clock);

        Self {
            restore_uc,
            plain_uc,
            touch_uc,
        }
    }

    #[instrument(skip_all, fields(entry_id = %entry_id))]
    pub async fn restore_entry(&self, entry_id: &str) -> Result<(), ClipboardRestoreError> {
        let parsed_id = EntryId::from(entry_id);

        self.restore_uc
            .execute(&parsed_id)
            .await
            .map_err(|err| map_restore_error(err, entry_id))?;

        self.touch_after_restore(&parsed_id, entry_id).await;
        Ok(())
    }

    /// 「以纯文本形式」恢复条目到系统剪贴板。
    ///
    /// 流程：先尝试 plain 路径（只写 `text/plain` 表示）；条目没有任何可用的
    /// plain 表示时，静默降级到 `restore_entry` 同等的多格式恢复路径——用户
    /// 视角就是"按 Option 没生效"，不弹错误。
    ///
    /// LRU 触摸：无论走 plain 路径还是降级路径，恢复成功后都调用
    /// `TouchClipboardEntryUseCase`，行为与 `restore_entry` 一致。
    #[instrument(skip_all, fields(entry_id = %entry_id))]
    pub async fn restore_entry_as_plain_text(
        &self,
        entry_id: &str,
    ) -> Result<(), ClipboardRestoreError> {
        let parsed_id = EntryId::from(entry_id);

        let outcome = self
            .plain_uc
            .execute(&parsed_id)
            .await
            .map_err(|err| map_restore_error(err, entry_id))?;

        match outcome {
            PlainRestoreOutcome::Done => {
                self.touch_after_restore(&parsed_id, entry_id).await;
                Ok(())
            }
            PlainRestoreOutcome::NoPlainTextAvailable => {
                tracing::info!(
                    entry_id = %entry_id,
                    "restore_entry_as_plain_text: no plain rep available, falling back to multi-format restore"
                );
                self.restore_uc
                    .execute(&parsed_id)
                    .await
                    .map_err(|err| map_restore_error(err, entry_id))?;
                self.touch_after_restore(&parsed_id, entry_id).await;
                Ok(())
            }
        }
    }

    async fn touch_after_restore(&self, parsed_id: &EntryId, entry_id: &str) {
        if let Err(err) = self.touch_uc.execute(parsed_id).await {
            tracing::warn!(
                error = %err,
                entry_id = %entry_id,
                "touch_clipboard_entry failed after restore"
            );
        }
    }
}

/// Translate the typed `PayloadResolveError` carried inside the anyhow chain
/// into a stable application error. Orphaned / Lost are user-visible "content
/// gone" outcomes (→ 410 at the API layer); Integrity is a data-corruption
/// bug and stays Internal. Anything containing "not found" becomes NotFound.
fn map_restore_error(err: anyhow::Error, entry_id: &str) -> ClipboardRestoreError {
    if let Some(payload_err) = err.downcast_ref::<PayloadResolveError>() {
        match payload_err {
            PayloadResolveError::Orphaned { rep_id, state } => {
                return ClipboardRestoreError::PayloadUnavailable {
                    entry_id: entry_id.to_string(),
                    rep_id: rep_id.to_string(),
                    state: state.as_str().to_string(),
                };
            }
            PayloadResolveError::Lost { rep_id, .. } => {
                return ClipboardRestoreError::PayloadUnavailable {
                    entry_id: entry_id.to_string(),
                    rep_id: rep_id.to_string(),
                    state: "Lost".to_string(),
                };
            }
            PayloadResolveError::Integrity { .. } => {
                // fall through — internal bug, return as Internal(500)
            }
        }
    }

    let message = err.to_string();
    if message.to_lowercase().contains("not found") {
        ClipboardRestoreError::NotFound
    } else {
        ClipboardRestoreError::Internal(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::clipboard::PayloadAvailability;
    use uc_core::ids::RepresentationId;

    #[test]
    fn maps_orphaned_payload_to_payload_unavailable() {
        let err = anyhow::Error::new(PayloadResolveError::Orphaned {
            rep_id: RepresentationId::from("rep-orphan"),
            state: PayloadAvailability::Staged,
        });

        let mapped = map_restore_error(err, "entry-1");
        assert!(matches!(
            mapped,
            ClipboardRestoreError::PayloadUnavailable { ref entry_id, ref rep_id, ref state }
                if entry_id == "entry-1" && rep_id == "rep-orphan" && state == "Staged"
        ));
    }

    #[test]
    fn maps_lost_payload_to_payload_unavailable_with_lost_state() {
        let err = anyhow::Error::new(PayloadResolveError::Lost {
            rep_id: RepresentationId::from("rep-lost"),
            reason: "manual fixture".to_string(),
        });

        let mapped = map_restore_error(err, "entry-2");
        assert!(matches!(
            mapped,
            ClipboardRestoreError::PayloadUnavailable { ref state, .. } if state == "Lost"
        ));
    }

    #[test]
    fn maps_integrity_to_internal() {
        let err = anyhow::Error::new(PayloadResolveError::Integrity {
            rep_id: RepresentationId::from("rep-bad"),
            reason: "corrupt header".to_string(),
        });

        let mapped = map_restore_error(err, "entry-3");
        match mapped {
            ClipboardRestoreError::Internal(msg) => {
                assert!(msg.to_lowercase().contains("integrity") || msg.contains("corrupt"));
            }
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[test]
    fn maps_anyhow_with_not_found_substring_to_not_found() {
        let err = anyhow::anyhow!("Entry not found");
        let mapped = map_restore_error(err, "entry-4");
        assert!(matches!(mapped, ClipboardRestoreError::NotFound));
    }

    #[test]
    fn case_insensitive_not_found_match() {
        let err = anyhow::anyhow!("Selection NOT FOUND for entry");
        let mapped = map_restore_error(err, "entry-5");
        assert!(matches!(mapped, ClipboardRestoreError::NotFound));
    }

    #[test]
    fn unknown_anyhow_error_falls_back_to_internal() {
        let err = anyhow::anyhow!("write coordinator deadlocked");
        let mapped = map_restore_error(err, "entry-6");
        match mapped {
            ClipboardRestoreError::Internal(msg) => {
                assert_eq!(msg, "write coordinator deadlocked");
            }
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[test]
    fn payload_resolve_error_takes_precedence_over_not_found_substring() {
        // 即使 anyhow message 含 "not found", PayloadResolveError 仍优先映射
        let err = anyhow::Error::new(PayloadResolveError::Lost {
            rep_id: RepresentationId::from("rep-x"),
            reason: "Selection not found".to_string(),
        });
        let mapped = map_restore_error(err, "entry-7");
        assert!(matches!(
            mapped,
            ClipboardRestoreError::PayloadUnavailable { .. }
        ));
    }
}
