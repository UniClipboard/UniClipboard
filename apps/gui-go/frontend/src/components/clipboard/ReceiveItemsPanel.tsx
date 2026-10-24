import React from 'react'
import { useTranslation } from 'react-i18next'
import { useEntryReceive } from '@/hooks/useEntryReceive'
import { retryableItemIds, summarizeReceiveItems } from '@/lib/receive-items'
import { cn } from '@/lib/utils'

interface ReceiveItemsPanelProps {
  entryId: string
}

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

const STATE_TONE: Record<string, string> = {
  completed: 'text-emerald-600 dark:text-emerald-400',
  failed: 'text-destructive',
  cancelled: 'text-muted-foreground',
}

/**
 * What each file of a received entry did, next to a retry for the failed ones.
 * An aggregate failure never hides a file that was saved; a directory is one row
 * and is retried as a whole.
 */
const ReceiveItemsPanel: React.FC<ReceiveItemsPanelProps> = ({ entryId }) => {
  const { t } = useTranslation()
  const { snapshot, cancelIncomplete, retrying, retryMessage, retry } = useEntryReceive(entryId)

  if (!snapshot || snapshot.items.length === 0) return null

  const summary = summarizeReceiveItems(snapshot)
  const canRetry = retryableItemIds(snapshot).length > 0

  return (
    <section
      aria-label={t('clipboard.receive.title')}
      data-testid="receive-items"
      className="flex flex-col gap-2 rounded-[0.625rem] bg-muted px-3 py-2.5"
    >
      <header className="flex items-center justify-between gap-2">
        <span className="text-ui-caption font-medium">{t('clipboard.receive.title')}</span>
        <span className="text-ui-caption text-muted-foreground">
          {t('clipboard.receive.summary', {
            saved: summary.saved,
            total: snapshot.items.length,
          })}
        </span>
      </header>
      {cancelIncomplete && (
        <p role="status" className="text-ui-caption font-medium text-muted-foreground">
          {t('clipboard.receive.cancelIncomplete')}
        </p>
      )}
      <ul className="flex flex-col gap-1">
        {snapshot.items.map((item, index) => {
          const state = KNOWN_STATES.has(item.state) ? item.state : 'unknown'
          const kind = KNOWN_KINDS.has(item.kind) ? item.kind : 'unknown'
          const reason =
            item.failureReason && KNOWN_REASONS.has(item.failureReason)
              ? item.failureReason
              : item.failureReason
                ? 'unknown'
                : null
          return (
            <li
              key={item.itemId}
              data-testid="receive-item"
              data-state={item.state}
              className="flex items-center justify-between gap-2 text-ui-caption"
            >
              <span className="min-w-0 truncate">
                {t(`clipboard.receive.kind.${kind}`)} {index + 1}
              </span>
              <span className={cn('shrink-0 font-medium', STATE_TONE[state])}>
                {t(`clipboard.receive.state.${state}`)}
                {reason && state === 'failed' && (
                  <span className="font-normal text-muted-foreground">
                    {' '}
                    {t(`clipboard.receive.reason.${reason}`)}
                  </span>
                )}
              </span>
            </li>
          )
        })}
      </ul>
      {retryMessage && (
        <p role="status" className="text-ui-caption text-muted-foreground">
          {t(retryMessage.key)}
        </p>
      )}
      {canRetry && (
        <button
          type="button"
          disabled={retrying}
          onClick={() => void retry()}
          className={cn(
            'self-start rounded-md border border-border bg-background px-2.5 py-1 text-ui-caption font-medium transition-colors',
            'hover:bg-muted/60 disabled:cursor-not-allowed disabled:opacity-50'
          )}
        >
          {t('clipboard.receive.retry.action')}
        </button>
      )}
    </section>
  )
}

export default ReceiveItemsPanel
