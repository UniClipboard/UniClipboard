import { LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { OutboundProgressItemDto } from '@/api/generated/types.gen'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import { selectOutboundByEntry, selectOutboundList } from '@/store/slices/outboundProgressSlice'
import { formatFileSize } from '@/utils'

type Translate = (key: string, opts?: Record<string, unknown>) => string

/**
 * Only a live, running attempt animates. An unconfirmed or settled snapshot
 * never does: the host does not guess an outcome from a timer.
 */
function isRunning(snapshot: OutboundProgressSnapshot): boolean {
  return (
    snapshot.freshness !== 'unconfirmed' &&
    snapshot.freshness !== 'settled' &&
    (snapshot.state === 'in_progress' || snapshot.state === 'waiting')
  )
}

function tone(state: string): string {
  if (state === 'completed') return 'text-emerald-600 dark:text-emerald-400'
  if (state === 'failed' || state === 'partially_failed') return 'text-destructive'
  return 'text-muted-foreground'
}

/** Aggregate bytes; the total is unknown (not 0) unless every item reports one. */
function aggregateBytes(items: OutboundProgressItemDto[]): {
  done: number
  total: number | null
} {
  const done = items.reduce((sum, item) => sum + (item.completedBytes ?? 0), 0)
  const known = items.length > 0 && items.every(item => item.totalBytes != null)
  return {
    done,
    total: known ? items.reduce((sum, item) => sum + (item.totalBytes ?? 0), 0) : null,
  }
}

function itemDetail(item: OutboundProgressItemDto, t: Translate): string | null {
  if (item.failureReason) return t(`delivery.outbound.reason.${item.failureReason}`)
  if (item.cancellationReason) return t(`delivery.outbound.cancel.${item.cancellationReason}`)
  return null
}

function OutboundRow({ snapshot, name }: { snapshot: OutboundProgressSnapshot; name: string }) {
  const { t } = useTranslation()
  const items = snapshot.items ?? []
  const running = isRunning(snapshot)
  const { done, total } = aggregateBytes(items)
  const showBar = items.length > 0 && snapshot.state !== 'no_items'
  // Reaching the total never means completed; the batch state decides.
  const percent = total && total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null

  return (
    <li
      data-outbound-state={snapshot.state}
      data-outbound-freshness={snapshot.freshness}
      className="flex flex-col gap-1.5 rounded-lg border border-border bg-background px-3 py-2"
    >
      <div className="flex items-center gap-2 text-ui-caption">
        <span className="min-w-0 truncate font-medium">{name}</span>
        <span className="flex-1" />
        {running && <LoaderCircle className="size-3 animate-spin" aria-hidden="true" />}
        <span className={cn('shrink-0 font-semibold', tone(snapshot.state))}>
          {t(`delivery.outbound.state.${snapshot.state}`)}
        </span>
      </div>
      {showBar && (
        <div
          role="progressbar"
          aria-label={t('delivery.outbound.progressAria', { device: name })}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent ?? undefined}
          aria-valuetext={
            percent === null
              ? t('delivery.outbound.unknownTotal')
              : t('delivery.outbound.percent', { percent })
          }
          className="h-1 overflow-hidden rounded-full bg-muted"
        >
          {percent !== null ? (
            <div className="h-full bg-primary/60" style={{ width: `${percent}%` }} />
          ) : (
            <div className={cn('h-full w-1/3 bg-primary/40', running && 'animate-pulse')} />
          )}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-3 text-ui-caption text-muted-foreground">
        <span>
          {t('delivery.outbound.items', {
            completed: snapshot.itemsCompleted,
            total: snapshot.itemsTotal,
          })}
        </span>
        {items.length > 0 && (
          <span className="tabular-nums">
            {total !== null
              ? `${formatFileSize(done)} / ${formatFileSize(total)}`
              : formatFileSize(done)}
          </span>
        )}
      </div>
      {items.some(item => item.state === 'failed' || item.state === 'cancelled') && (
        <ul className="flex flex-col gap-0.5 text-ui-caption text-muted-foreground">
          {items
            .filter(item => item.state === 'failed' || item.state === 'cancelled')
            .map(item => (
              <li key={`${item.kind}-${item.index}`}>
                {t(`delivery.outbound.kind.${item.kind}`, { index: item.index + 1 })}
                {' · '}
                {t(`delivery.outbound.itemState.${item.state}`)}
                {itemDetail(item, t) ? ` · ${itemDetail(item, t)}` : ''}
              </li>
            ))}
        </ul>
      )}
    </li>
  )
}

/**
 * One row per target device for an entry, straight from the Engine snapshot.
 * Devices stay separate: a multi-device send is never folded into one status.
 */
export default function HistoryDetailOutbound({
  entryId,
  deviceNames,
}: {
  entryId: string
  deviceNames: Record<string, string>
}) {
  const { t } = useTranslation()
  const snapshots = selectOutboundList(
    useAppSelector(state => selectOutboundByEntry(state, entryId))
  )
  if (snapshots.length === 0) return null
  return (
    <ul className="flex flex-col gap-2" aria-label={t('delivery.outbound.listAria')}>
      {snapshots.map(snapshot => (
        <OutboundRow
          key={snapshot.targetDeviceId}
          snapshot={snapshot}
          name={deviceNames[snapshot.targetDeviceId] ?? snapshot.targetDeviceId.slice(0, 8)}
        />
      ))}
    </ul>
  )
}
