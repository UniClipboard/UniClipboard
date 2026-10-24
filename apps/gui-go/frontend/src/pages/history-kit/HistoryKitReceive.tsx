import { useTranslation } from 'react-i18next'
import { useEntryReceive } from '@/hooks/useEntryReceive'
import { retryableItemIds, summarizeReceiveItems } from '@/lib/receive-items'
import type { ReceiveItem } from '@/lib/receive-items'
import { Button, InlineNotice, InsetList, Text, type InsetListRow, type StatusTone } from '@/ui-kit'

const KNOWN_STATES = new Set(['pending', 'transferring', 'completed', 'failed', 'cancelled'])
const KNOWN_KINDS = new Set(['file', 'representation', 'directory_unit'])
const KNOWN_REASONS = new Set([
  'network_unavailable',
  'timed_out',
  'access_denied',
  'storage_unavailable',
  'integrity_check_failed',
  'interrupted',
  'unknown',
])

const TONE: Record<string, StatusTone> = {
  completed: 'positive',
  failed: 'critical',
  transferring: 'info',
  pending: 'info',
}

/**
 * What each file of a received entry did, with a retry for the failed ones. An
 * aggregate failure never hides a file that was saved, and a directory is one row
 * that is retried as a whole. Results come from the Engine's snapshot; this view
 * only lays them out on the shared kit components.
 */
export function HistoryKitReceive({ entryId }: { entryId: string }) {
  const { t } = useTranslation()
  const { snapshot, cancelIncomplete, retrying, retryMessage, retry } = useEntryReceive(entryId)
  if (!snapshot || snapshot.items.length === 0) return null

  const summary = summarizeReceiveItems(snapshot)
  const canRetry = retryableItemIds(snapshot).length > 0
  const rows: InsetListRow[] = snapshot.items.map((item, index) => ({
    id: item.itemId,
    title: `${t(`clipboard.receive.kind.${KNOWN_KINDS.has(item.kind) ? item.kind : 'unknown'}`)} ${index + 1}`,
    detail: describeState(item, t),
    tone: TONE[item.state] ?? 'neutral',
  }))

  return (
    <section className="flex shrink-0 flex-col gap-1.5" aria-label={t('clipboard.receive.title')}>
      <div className="flex items-baseline gap-2">
        <Text role="headline">{t('clipboard.receive.title')}</Text>
        <Text role="caption" tone="secondary">
          {t('clipboard.receive.summary', { saved: summary.saved, total: snapshot.items.length })}
        </Text>
      </div>
      {cancelIncomplete && (
        <InlineNotice tone="info" title={t('clipboard.receive.cancelIncomplete')} />
      )}
      <InsetList label={t('clipboard.receive.title')} rows={rows} />
      {retryMessage && <InlineNotice tone={retryMessage.tone} title={t(retryMessage.key)} />}
      {canRetry && (
        <div>
          <Button size="small" busy={retrying} onClick={() => void retry()}>
            {t('clipboard.receive.retry.action')}
          </Button>
        </div>
      )}
    </section>
  )
}

function describeState(item: ReceiveItem, t: (key: string) => string): string {
  const state = KNOWN_STATES.has(item.state) ? item.state : 'unknown'
  const label = t(`clipboard.receive.state.${state}`)
  if (state !== 'failed' || !item.failureReason) return label
  const reason = KNOWN_REASONS.has(item.failureReason) ? item.failureReason : 'unknown'
  return `${label} · ${t(`clipboard.receive.reason.${reason}`)}`
}
