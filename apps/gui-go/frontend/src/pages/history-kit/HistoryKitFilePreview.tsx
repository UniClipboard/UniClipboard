import { useTranslation } from 'react-i18next'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import FilePreview from '@/components/clipboard/preview-renderers/FilePreview'
import FilePreviewProgress from '@/components/clipboard/preview-renderers/FilePreviewProgress'
import { useCancelEntryTransfer } from '@/hooks/useCancelEntryTransfer'
import type { ClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryReceive } from '@/hooks/useEntryReceive'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { receiveFileContent, receiveFileRows } from '@/lib/receive-file-progress'
import { retryableItemIds, summarizeReceiveItems } from '@/lib/receive-items'
import type { ReceiveItem } from '@/lib/receive-items'
import { useAppSelector } from '@/store/hooks'
import { selectOutboundByEntry, selectOutboundList } from '@/store/slices/outboundProgressSlice'
import {
  Button,
  InlineNotice,
  InsetList,
  PreviewFrame,
  Text,
  type InsetListRow,
  type StatusTone,
} from '@/ui-kit'
import { HistoryKitFileOutbound } from './HistoryKitFileOutbound'
import { HistoryKitFileReceiveProgress } from './HistoryKitFileReceiveProgress'

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
  delivery,
}: {
  item: DisplayClipboardItem
  state: ClipboardPreviewState
  delivery?: EntryDeliveryView | null
}) {
  const { t } = useTranslation()
  const outbound = selectOutboundList(
    useAppSelector(store => selectOutboundByEntry(store, item.id))
  )
  const hasFileProgress =
    !item.isDirectory &&
    outbound.some(snapshot => snapshot.items?.some(item => item.kind === 'file'))
  const deviceNames = Object.fromEntries(
    (delivery?.deliveries ?? []).map(target => [
      target.targetDeviceId,
      target.targetDeviceName ?? target.targetDeviceId.slice(0, 8),
    ])
  )
  const { snapshot, historyFiles, cancelIncomplete, retrying, retryMessage, retry } =
    useEntryReceive(item.id)
  const { cancelling, cancel } = useCancelEntryTransfer(item.id, state.transfer)
  const pending = useAppSelector(store =>
    store.clipboard.pendingItems.find(entry => entry.entryId === item.id)
  )
  const receiveRows = snapshot ? receiveFileRows(snapshot) : []
  const hasReceiveFiles = receiveRows.length > 0
  const receiveItem =
    snapshot && hasReceiveFiles
      ? {
          ...item,
          content: receiveFileContent(
            snapshot,
            historyFiles,
            pending?.attemptId === snapshot.attemptId ? pending.filenames : [],
            index => t('delivery.outbound.kind.file', { index })
          ),
        }
      : item
  const source =
    delivery?.source.tag === 'remote'
      ? (delivery.source.deviceName ?? delivery.source.deviceId.slice(0, 8))
      : item.device
  const directory = snapshot?.items.find(item => item.content?.kind === 'directory_unit')
  const summary = snapshot && snapshot.items.length > 0 ? summarizeReceiveItems(snapshot) : null
  const canRetry = snapshot && retryableItemIds(snapshot).length > 0
  // Only explicit content locators attach to file rows; unknown and representation results stay separate.
  const rows: InsetListRow[] = (snapshot?.items ?? []).flatMap((item, index) =>
    item.state === 'completed' ||
    item.content?.kind === 'file' ||
    item.content?.kind === 'directory_unit'
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
          item={receiveItem}
          preview={state.preview}
          effectiveStatus={state.effectiveStatus}
          entryStatus={state.entryStatus}
          transfer={state.transfer}
          summary={summaryLabel}
          renderFileProgress={
            hasReceiveFiles
              ? (index, name) => (
                  <HistoryKitFileReceiveProgress
                    item={receiveRows[index].item}
                    index={receiveRows[index].index}
                    name={name}
                    source={source}
                  />
                )
              : hasFileProgress
                ? (index, name) => (
                    <HistoryKitFileOutbound
                      index={index}
                      fileName={name}
                      snapshots={outbound}
                      deviceNames={deviceNames}
                    />
                  )
                : undefined
          }
        >
          {directory && (
            <div className="px-4 py-3">
              <Text role="caption">{t('clipboard.receive.wholeFileSet')}</Text>
              <HistoryKitFileReceiveProgress
                item={directory}
                name={t('clipboard.receive.wholeFileSet')}
                source={source}
              />
            </div>
          )}
          {!hasReceiveFiles &&
            !directory &&
            state.effectiveStatus === 'transferring' &&
            state.transfer?.status === 'active' && (
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
