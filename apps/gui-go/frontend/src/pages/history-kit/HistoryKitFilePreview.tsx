import { useTranslation } from 'react-i18next'
import FilePreview from '@/components/clipboard/preview-renderers/FilePreview'
import FilePreviewProgress from '@/components/clipboard/preview-renderers/FilePreviewProgress'
import { useCancelEntryTransfer } from '@/hooks/useCancelEntryTransfer'
import type { ClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryReceive } from '@/hooks/useEntryReceive'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { retryableItemIds, summarizeReceiveItems } from '@/lib/receive-items'
import type { ReceiveItem } from '@/lib/receive-items'
import {
  Button,
  InlineNotice,
  InsetList,
  PreviewFrame,
  Text,
  type InsetListRow,
  type StatusTone,
} from '@/ui-kit'

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

/** One file surface owns content, aggregate receive results and retry feedback. */
export function HistoryKitFilePreview({
  item,
  state,
}: {
  item: DisplayClipboardItem
  state: ClipboardPreviewState
}) {
  const { t } = useTranslation()
  const { snapshot, cancelIncomplete, retrying, retryMessage, retry } = useEntryReceive(item.id)
  const { cancelling, cancel } = useCancelEntryTransfer(item.id, state.transfer)
  const summary = snapshot && snapshot.items.length > 0 ? summarizeReceiveItems(snapshot) : null
  const canRetry = snapshot && retryableItemIds(snapshot).length > 0
  // Receive IDs are opaque. Until the Engine exposes a file association, keep
  // unresolved records separate from file rows instead of joining by array order.
  const rows: InsetListRow[] = (snapshot?.items ?? []).flatMap((item, index) =>
    item.state === 'completed'
      ? []
      : [
          {
            id: item.itemId,
            title: `${t(`clipboard.receive.kind.${KNOWN_KINDS.has(item.kind) ? item.kind : 'unknown'}`)} ${index + 1}`,
            detail: describeState(item, t),
            tone: TONE[item.state] ?? 'neutral',
          },
        ]
  )
  const canCancel =
    state.effectiveStatus === 'transferring' &&
    state.transfer?.status === 'active' &&
    state.transfer.direction === 'receiving'
  const summaryLabel =
    summary && snapshot
      ? [
          t('clipboard.receive.summary', {
            saved: summary.saved,
            total: snapshot.items.length,
          }),
          summary.failed > 0 ? t('clipboard.receive.failedCount', { count: summary.failed }) : null,
          summary.cancelled > 0
            ? t('clipboard.receive.cancelledCount', { count: summary.cancelled })
            : null,
        ]
          .filter(Boolean)
          .join(' · ')
      : undefined

  return (
    <PreviewFrame kind="file">
      <div data-testid="file-preview">
        <FilePreview
          item={item}
          preview={state.preview}
          effectiveStatus={state.effectiveStatus}
          entryStatus={state.entryStatus}
          transfer={state.transfer}
          summary={summaryLabel}
        >
          {state.effectiveStatus === 'transferring' && state.transfer?.status === 'active' && (
            <FilePreviewProgress transfer={state.transfer} />
          )}
        </FilePreview>
        {(cancelIncomplete || retryMessage || canRetry || canCancel || rows.length > 0) && (
          <div className="flex flex-col gap-3 border-t border-border/50 px-4 py-3">
            {cancelIncomplete && (
              <InlineNotice tone="info" title={t('clipboard.receive.cancelIncomplete')} />
            )}
            {rows.length > 0 && (
              <details>
                <summary className="cursor-pointer text-ui-body">
                  {t('clipboard.receive.details')}
                </summary>
                <div className="mt-3 flex flex-col gap-2">
                  <Text role="caption" tone="secondary">
                    {t('clipboard.receive.unlinkedDetails')}
                  </Text>
                  <InsetList label={t('clipboard.receive.title')} rows={rows} />
                </div>
              </details>
            )}
            {retryMessage && <InlineNotice tone={retryMessage.tone} title={t(retryMessage.key)} />}
            {(canRetry || canCancel) && (
              <div className="flex flex-wrap gap-2">
                {canRetry && (
                  <Button size="small" busy={retrying} onClick={() => void retry()}>
                    {t('clipboard.receive.retry.action')}
                  </Button>
                )}
                {canCancel && (
                  <Button size="small" busy={cancelling} onClick={() => void cancel()}>
                    {t('clipboard.transfer.cancel')}
                  </Button>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </PreviewFrame>
  )
}

function describeState(item: ReceiveItem, t: (key: string) => string): string {
  const state = KNOWN_STATES.has(item.state) ? item.state : 'unknown'
  const label = t(`clipboard.receive.state.${state}`)
  if (state !== 'failed' || !item.failureReason) return label
  const reason = KNOWN_REASONS.has(item.failureReason) ? item.failureReason : 'unknown'
  return `${label} · ${t(`clipboard.receive.reason.${reason}`)}`
}
