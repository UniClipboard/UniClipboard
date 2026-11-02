import type { OutboundProgressItemDto } from '@/api/generated/types.gen'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import type { EntryDeliveryStatusView } from '@/api/tauri-command/clipboard_delivery'
import { isOutboundRunning } from '@/store/slices/outboundProgressSlice'
import type { StatusTone, TransferStatusItem } from '@/ui-kit'
import { formatFileSize } from '@/utils'

export function transferBytes(items: OutboundProgressItemDto[]) {
  const knownDone = items.length > 0 && items.every(item => item.completedBytes != null)
  const done = items.reduce((sum, item) => sum + (item.completedBytes ?? 0), 0)
  const total =
    items.length && items.every(item => item.totalBytes != null)
      ? items.reduce((sum, item) => sum + (item.totalBytes ?? 0), 0)
      : null
  return {
    done,
    knownDone,
    total,
    percent:
      knownDone && total != null && total > 0
        ? Math.min(100, Math.max(0, Math.round((done / total) * 100)))
        : null,
  }
}

/** The retained batch owns file completion; a delivery receipt alone does not. */
export function deliveryStatus(
  status: EntryDeliveryStatusView,
  snapshot?: OutboundProgressSnapshot,
  busy = false
) {
  const state = snapshot?.state === 'no_items' ? undefined : snapshot?.state
  const running = busy || (snapshot != null && isOutboundRunning(snapshot))
  const unconfirmed = state === 'in_progress' || state === 'waiting'
  const effectiveState =
    state && (snapshot?.freshness === 'unconfirmed' || (unconfirmed && !running))
      ? 'unconfirmed'
      : state
  const complete =
    !busy &&
    (state
      ? effectiveState === 'completed'
      : status.tag === 'delivered' || status.tag === 'duplicate')
  const failed =
    !busy &&
    (state
      ? effectiveState === 'failed' || effectiveState === 'partially_failed'
      : status.tag === 'failed')
  const tone: StatusTone = running
    ? 'info'
    : complete
      ? 'positive'
      : failed
        ? 'critical'
        : 'neutral'
  return { running, complete, failed, tone, state: effectiveState }
}

type Translate = (key: string, options?: Record<string, unknown>) => string

/** Translate Engine data once; skins own layout, controls and all visual states. */
export function transferDetails(
  snapshot: OutboundProgressSnapshot | undefined,
  fileNames: readonly string[] | undefined,
  t: Translate
): { summary?: string; items: TransferStatusItem[] } {
  const items = (snapshot?.items ?? []).map(item => {
    const state = snapshot?.freshness === 'unconfirmed' ? 'unconfirmed' : item.state
    const tone: StatusTone = ['waiting', 'transferring', 'processing', 'retrying'].includes(state)
      ? 'info'
      : state === 'completed'
        ? 'positive'
        : state === 'failed'
          ? 'critical'
          : 'neutral'
    const label =
      (item.kind === 'file' && fileNames?.[item.index]) ||
      t(`delivery.outbound.kind.${item.kind}`, {
        index: item.index + 1,
        defaultValue: t('delivery.outbound.kind.unknown', { index: item.index + 1 }),
      })
    const { done, total, percent } = transferBytes([item])
    const reason = item.failureReason
      ? t(`delivery.outbound.reason.${item.failureReason}`, {
          defaultValue: t('delivery.outbound.reason.unknown'),
        })
      : item.cancellationReason
        ? t(`delivery.outbound.cancel.${item.cancellationReason}`, {
            defaultValue: t('delivery.outbound.cancel.unknown'),
          })
        : undefined
    return {
      id: `${item.kind}-${item.index}`,
      label,
      status: t(`delivery.outbound.itemState.${state}`, {
        defaultValue: t('delivery.outbound.state.unconfirmed'),
      }),
      tone,
      value: item.completedBytes == null ? undefined : (percent ?? undefined),
      bytes: `${item.completedBytes == null ? t('delivery.outbound.unknownBytes') : formatFileSize(done)} / ${total == null ? t('delivery.outbound.unknownTotal') : formatFileSize(total)}`,
      reason,
    }
  })
  if (!snapshot || snapshot.state === 'no_items') return { items }
  const counts = t('delivery.outbound.items', {
    completed: snapshot.itemsCompleted,
    total: snapshot.itemsTotal,
  })
  const { done, total, knownDone } = transferBytes(snapshot.items ?? [])
  return {
    items,
    summary: items.length
      ? `${counts} · ${knownDone ? formatFileSize(done) : t('delivery.outbound.unknownBytes')} / ${total == null ? t('delivery.outbound.unknownTotal') : formatFileSize(total)}`
      : counts,
  }
}
