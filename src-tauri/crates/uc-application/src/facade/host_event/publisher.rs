use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use async_trait::async_trait;
use tracing::warn;

/// 显式恢复 poisoned mutex 守卫,并 log 警告。
///
/// `unwrap_or_else(|p| p.into_inner())` 直接吞 poison 会让 invariant 违
/// 反静默,排障时完全找不到信号。这里集中加 warn 让"前一次 panic 留下
/// 了不一致状态"在日志里有据可查。`context` 标明发生位置(锁名),便于
/// grep。
#[inline]
fn recover_poisoned<T>(poisoned: PoisonError<T>, context: &'static str) -> T {
    warn!(
        context,
        "host event publisher: lock poisoned, recovering inner state (a prior panic likely left invariants broken)"
    );
    poisoned.into_inner()
}
use uc_core::file_transfer::{
    FileTransferCancellationReason, FileTransferEvent, FileTransferEventPublisherPort,
    FileTransferFailureReason,
};
use uc_core::ports::file_transfer_repository::FileTransferRepositoryPort;

use super::{HostEvent, HostEventBus, OutboundEntryIdCache, TransferHostEvent};

/// mobile_lan PUT /file handler 在 SyncDoc apply 之前不知道真实 entry_id,
/// 用 `mobile-pending:<transfer_id>` 占位写到 receiver-side projection。
/// publisher 看到这个前缀就视作"尚未 link",buffered 阶段的 StatusChanged
/// 不发 WS,延后到 `link_transfer_to_entry` 之后再用真实 entry_id 补发。
const PLACEHOLDER_ENTRY_ID_PREFIX: &str = "mobile-pending:";

fn is_placeholder_entry_id(entry_id: &str) -> bool {
    entry_id.starts_with(PLACEHOLDER_ENTRY_ID_PREFIX)
}

/// 在 buffered 阶段被暂缓的状态变更:link 之后补发用。
///
/// 目前只 buffer `Started`(语义上 = `transferring` 入口)。Progress 不
/// 经过 publish_status_change,自然 entry_id=None 直接发;Completed /
/// Failed / Cancelled 总是发生在 link 之后,resolve_entry_id 能拿真实
/// entry_id,不需要 buffer。
#[derive(Debug, Clone)]
struct PendingStatusChange {
    status: String,
    reason: Option<String>,
}

pub struct FileTransferHostEventPublisher {
    bus: Arc<HostEventBus>,
    file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
    outbound_entry_cache: Arc<OutboundEntryIdCache>,
    /// transfer_id → 暂存的 status_changed,等 link 后补发。
    pending_status: Arc<Mutex<HashMap<String, PendingStatusChange>>>,
    /// 已经为占位 transfer 发过 Progress(entry_id=None)的 transfer_id 集合,
    /// 仅用于诊断日志去重(每条 transfer 只 warn 一次"buffered 阶段 progress
    /// 没有 entry 链接")。
    progress_no_entry_warned: Arc<Mutex<HashSet<String>>>,
}

impl FileTransferHostEventPublisher {
    pub fn new(
        bus: Arc<HostEventBus>,
        file_transfer_repo: Arc<dyn FileTransferRepositoryPort>,
        outbound_entry_cache: Arc<OutboundEntryIdCache>,
    ) -> Self {
        Self {
            bus,
            file_transfer_repo,
            outbound_entry_cache,
            pending_status: Arc::new(Mutex::new(HashMap::new())),
            progress_no_entry_warned: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Resolve a real entry_id for the given transfer.
    ///
    /// projection 行的 entry_id 可能是占位符(`mobile-pending:...`),那种情
    /// 况视为"尚未 link",返回 `None`,调用方决定 skip 或 buffer。
    async fn resolve_entry_id(&self, transfer_id: &str) -> Option<String> {
        let resolved = match self
            .file_transfer_repo
            .get_entry_id_for_transfer(transfer_id)
            .await
        {
            Ok(Some(entry_id)) => Some(entry_id),
            Ok(None) => None,
            Err(err) => {
                warn!(error = %err, transfer_id, "failed to resolve entry_id from projection");
                None
            }
        };

        let resolved = resolved.or_else(|| self.outbound_entry_cache.get(transfer_id));

        resolved.filter(|id| !is_placeholder_entry_id(id))
    }

    fn emit(&self, event: HostEvent) {
        self.bus.emit_or_warn(event);
    }

    /// 在 `link_transfer_to_entry` 成功之后调用,把 buffered 阶段暂存的
    /// `StatusChanged transferring` 用真实 entry_id 补发出去。
    ///
    /// 后续到来的 `Completed` / `Failed` 通过 [`publish_status_change`] 自
    /// 然能 resolve 到真实 entry_id,不需要 buffer。
    pub async fn flush_pending_status_after_link(&self, transfer_id: &str) {
        let pending = self
            .pending_status
            .lock()
            .unwrap_or_else(|p| recover_poisoned(p, "pending_status"))
            .remove(transfer_id);
        let Some(pending) = pending else {
            return;
        };
        let Some(entry_id) = self.resolve_entry_id(transfer_id).await else {
            warn!(
                transfer_id,
                "flush_pending_status_after_link: still no real entry_id; dropping buffered status"
            );
            return;
        };
        self.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
            transfer_id: transfer_id.to_string(),
            entry_id,
            status: pending.status,
            reason: pending.reason,
        }));
    }
}

#[async_trait]
impl FileTransferEventPublisherPort for FileTransferHostEventPublisher {
    async fn publish(&self, event: FileTransferEvent) -> Result<()> {
        match event {
            FileTransferEvent::Started { transfer_id, .. } => {
                self.publish_status_change(&transfer_id, "transferring", None, "Started")
                    .await;
            }
            FileTransferEvent::Progress {
                transfer_id,
                peer_id,
                progress,
            } => {
                let entry_id = self.resolve_entry_id(&transfer_id).await;
                if entry_id.is_none() {
                    let mut warned = self
                        .progress_no_entry_warned
                        .lock()
                        .unwrap_or_else(|p| recover_poisoned(p, "progress_no_entry_warned"));
                    if warned.insert(transfer_id.clone()) {
                        warn!(
                            transfer_id = %transfer_id,
                            "buffered-phase progress: no real entry_id; front-end indexes via transferId only"
                        );
                    }
                }
                self.emit(HostEvent::Transfer(TransferHostEvent::Progress {
                    transfer_id,
                    entry_id,
                    peer_id,
                    direction: progress.direction,
                    bytes_transferred: progress.bytes_transferred,
                    total_bytes: progress.total_bytes,
                }));
            }
            FileTransferEvent::Completed { transfer_id, .. } => {
                self.publish_status_change(&transfer_id, "completed", None, "Completed")
                    .await;
            }
            FileTransferEvent::Failed {
                transfer_id,
                reason,
                detail,
                ..
            } => {
                let reason_label = Some(format_failure_reason(reason, detail.as_deref()));
                self.publish_status_change(&transfer_id, "failed", reason_label, "Failed")
                    .await;
            }
            FileTransferEvent::Cancelled {
                transfer_id,
                reason,
                ..
            } => {
                let reason_label = Some(cancellation_reason_label(reason).to_string());
                self.publish_status_change(&transfer_id, "cancelled", reason_label, "Cancelled")
                    .await;
            }
        }
        Ok(())
    }
}

impl FileTransferHostEventPublisher {
    async fn publish_status_change(
        &self,
        transfer_id: &str,
        status: &str,
        reason: Option<String>,
        event_kind: &'static str,
    ) {
        match self.resolve_entry_id(transfer_id).await {
            Some(entry_id) => {
                self.emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                    transfer_id: transfer_id.to_string(),
                    entry_id,
                    status: status.to_string(),
                    reason,
                }));
            }
            None => {
                // 还没 link(常见于 mobile_lan buffered 阶段的 Started):暂存
                // `Started`,等到 link_transfer_to_entry 之后由
                // `flush_pending_status_after_link` 补发。其他事件类型
                // (Completed / Failed / Cancelled)在 buffered 阶段拿不到真
                // 实 entry_id 是异常路径,只 warn 不 buffer —— 例如 PUT body
                // 中断时调 fail,这种 transfer 没产生真实 entry,WS 上前端也
                // 没行可显示,store 留 Failed 事件即可,timeout sweep 兜底。
                if event_kind == "Started" {
                    self.pending_status
                        .lock()
                        .unwrap_or_else(|p| recover_poisoned(p, "pending_status"))
                        .insert(
                            transfer_id.to_string(),
                            PendingStatusChange {
                                status: status.to_string(),
                                reason,
                            },
                        );
                } else {
                    warn!(
                        transfer_id,
                        event_kind, "no entry_id resolved; skipping host status event"
                    );
                }
            }
        }
    }
}

fn failure_reason_label(reason: FileTransferFailureReason) -> &'static str {
    match reason {
        FileTransferFailureReason::NetworkUnavailable => "network_unavailable",
        FileTransferFailureReason::TimedOut => "timed_out",
        FileTransferFailureReason::AccessDenied => "access_denied",
        FileTransferFailureReason::StorageUnavailable => "storage_unavailable",
        FileTransferFailureReason::IntegrityCheckFailed => "integrity_check_failed",
        FileTransferFailureReason::Unknown => "unknown",
    }
}

fn format_failure_reason(reason: FileTransferFailureReason, detail: Option<&str>) -> String {
    let label = failure_reason_label(reason);
    match detail.map(str::trim).filter(|s| !s.is_empty()) {
        Some(detail) => format!("{label}: {detail}"),
        None => label.to_string(),
    }
}

fn cancellation_reason_label(reason: FileTransferCancellationReason) -> &'static str {
    match reason {
        FileTransferCancellationReason::LocalUser => "local_user",
        FileTransferCancellationReason::RemotePeer => "remote_peer",
        FileTransferCancellationReason::Replaced => "replaced",
        FileTransferCancellationReason::Timeout => "timeout",
        FileTransferCancellationReason::Unknown => "unknown",
    }
}
