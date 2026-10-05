import { LoaderCircle, RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type {
  EntryDeliveryStatusView,
  EntryDeliveryTargetView,
  EntryDeliveryView,
} from '@/api/tauri-command/clipboard_delivery'
import {
  deviceLabel,
  getStatusLabel,
  renderStatusTone,
} from '@/components/clipboard/entry-delivery-labels'
import { summarize, syncSummaryView } from '@/components/clipboard/EntryDeliveryBadge'
import { useResendAction } from '@/hooks/useResendAction'
import { cn } from '@/lib/utils'
import { DETAIL_SECTION_LABEL } from './detail-styles'

function statusDot(status: EntryDeliveryStatusView): string {
  switch (status.tag) {
    case 'delivered':
    case 'duplicate':
      return 'bg-emerald-500'
    case 'failed':
      return 'bg-destructive'
    default:
      return 'bg-muted-foreground/40'
  }
}

// A delivered peer has nothing to retry; every other status can be resent.
function isRetryable(status: EntryDeliveryStatusView): boolean {
  return status.tag !== 'delivered' && status.tag !== 'duplicate'
}

interface PeerChipProps {
  target: EntryDeliveryTargetView
  entryId: string
  resendable: boolean
  action: ReturnType<typeof useResendAction>
}

function PeerChip({ target, entryId, resendable, action }: PeerChipProps) {
  const { t } = useTranslation()
  const { status } = target
  const name = deviceLabel(target.targetDeviceName, target.targetDeviceId)
  const delivered = !isRetryable(status)
  const failed = status.tag === 'failed'
  const inFlight = action.isPeerInFlight(entryId, target.targetDeviceId)
  const busy = inFlight || action.isEntryInFlight(entryId)
  const canResend = resendable && !delivered

  return (
    <li
      data-status={status.tag}
      className={cn(
        'inline-flex h-7.5 max-w-full items-center gap-1.5 rounded-full border pl-2.5 text-ui-caption',
        canResend ? 'pr-1' : 'pr-2.5',
        failed ? 'border-destructive/30 bg-destructive/5' : 'border-border bg-background'
      )}
    >
      <span className={cn('size-2 shrink-0 rounded-full', statusDot(status))} aria-hidden="true" />
      <span className="min-w-0 truncate font-medium">{name}</span>
      {/* Settled peers stay quiet; only a pending, offline or failed one explains itself. */}
      {!delivered && (
        <span className={cn('shrink-0', renderStatusTone(status).label)}>
          {getStatusLabel(status, t)}
        </span>
      )}
      {canResend && (
        <button
          type="button"
          aria-label={t('delivery.resend.button.peerAria', { device: name })}
          disabled={busy}
          onClick={() => void action.resendToPeer(entryId, target.targetDeviceId)}
          data-resend-peer={target.targetDeviceId}
          className="inline-flex size-5.5 shrink-0 items-center justify-center rounded-full text-foreground transition-colors hover:bg-muted disabled:text-muted-foreground/40 disabled:hover:bg-transparent"
        >
          {inFlight ? (
            <LoaderCircle className="size-3 animate-spin" aria-hidden="true" />
          ) : (
            <RefreshCw className="size-3" aria-hidden="true" />
          )}
        </button>
      )}
    </li>
  )
}

/**
 * Sync status block: the aggregate result next to the heading, then one chip
 * per paired device. Settled devices show only a dot and a name; the ones that
 * still need attention carry their status and a resend button.
 */
export default function HistoryDetailDelivery({ delivery }: { delivery: EntryDeliveryView }) {
  const { t } = useTranslation()
  const action = useResendAction()
  const { source, deliveries, entryId } = delivery
  const resendable = source.tag === 'local'
  const summary = source.tag === 'historical' ? null : summarize(deliveries)
  const view = summary ? syncSummaryView(summary, t) : null
  const note =
    source.tag === 'historical'
      ? t('delivery.list.historical')
      : deliveries.length === 0
        ? t('delivery.list.noPeers')
        : null
  const entryInFlight = action.isEntryInFlight(entryId)
  const retryAll = resendable && deliveries.some(d => isRetryable(d.status))

  return (
    <section className="flex shrink-0 flex-col gap-2.5" aria-label={t('delivery.section.aria')}>
      <div className="flex min-h-6 items-center gap-3">
        <h3 className={DETAIL_SECTION_LABEL}>{t('delivery.list.title')}</h3>
        {view && (
          <span
            data-delivery-summary={summary}
            className={cn(
              'inline-flex items-center gap-1.5 text-ui-caption font-semibold',
              view.tone
            )}
          >
            <view.Icon className={cn('size-3.5', view.spin && 'animate-spin')} aria-hidden="true" />
            {view.label}
          </span>
        )}
        <span className="flex-1" />
        {retryAll && (
          <button
            type="button"
            disabled={entryInFlight}
            aria-label={t('delivery.resend.button.entryAria')}
            onClick={() => void action.resendAll(entryId)}
            data-resend-entry=""
            className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-ui-caption font-medium text-foreground transition-colors hover:bg-muted disabled:text-muted-foreground/40 disabled:hover:bg-transparent"
          >
            {entryInFlight ? (
              <LoaderCircle className="size-3 animate-spin" aria-hidden="true" />
            ) : (
              <RefreshCw className="size-3" aria-hidden="true" />
            )}
            {entryInFlight
              ? t('delivery.resend.button.pending')
              : t('delivery.resend.button.entry')}
          </button>
        )}
      </div>
      {note ? (
        <p className="text-ui-caption text-muted-foreground">{note}</p>
      ) : (
        <ul className="flex flex-wrap gap-2">
          {deliveries.map(target => (
            <PeerChip
              key={target.targetDeviceId}
              target={target}
              entryId={entryId}
              resendable={resendable}
              action={action}
            />
          ))}
        </ul>
      )}
    </section>
  )
}
