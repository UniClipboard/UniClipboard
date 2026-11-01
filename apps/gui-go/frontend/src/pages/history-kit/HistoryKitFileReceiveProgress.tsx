import { useTranslation } from 'react-i18next'
import { receiveTransferMetrics } from '@/lib/receive-file-progress'
import type { ReceiveItem } from '@/lib/receive-items'
import { ProgressBar, Text } from '@/ui-kit'
import { formatFileSize } from '@/utils'

/** Network bytes never promote a receive item across the Engine's save boundary. */
export function HistoryKitFileReceiveProgress({
  item,
  name,
  source,
  index,
}: {
  item: ReceiveItem
  name: string
  source?: string | null
  index?: number
}) {
  const { t } = useTranslation()
  const { received: bytes, total, percent, showBar } = receiveTransferMetrics(item)
  return (
    <div
      data-receive-file-state={item.state}
      data-receive-file-index={index}
      className="flex min-w-0 flex-col gap-1.5"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        {source && <Text role="caption">{source}</Text>}
        <Text role="caption" tone={item.state === 'failed' ? 'critical' : 'secondary'}>
          {t(`clipboard.receive.state.${item.state}`)}
          {percent !== undefined && ` · ${Math.round(percent)}%`}
        </Text>
      </div>
      {showBar && (
        <ProgressBar
          label={t('clipboard.receive.fileProgressAria', { file: name })}
          value={percent}
        />
      )}
      {item.transfer ? (
        <Text role="caption" tone="secondary">
          {bytes !== null ? formatFileSize(Number(bytes)) : t('delivery.outbound.unknownBytes')}
          {' / '}
          {total !== null ? formatFileSize(Number(total)) : t('delivery.outbound.unknownTotal')}
        </Text>
      ) : (
        <Text role="caption" tone="secondary">
          {t('clipboard.receive.progressUnavailable')}
        </Text>
      )}
      {item.failureReason && (
        <Text role="caption" tone="critical">
          {t(`clipboard.receive.reason.${item.failureReason}`)}
        </Text>
      )}
    </div>
  )
}
