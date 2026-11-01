import { useTranslation } from 'react-i18next'
import type { OutboundProgressItemDto } from '@/api/generated/types.gen'
import { ProgressBar, Text } from '@/ui-kit'
import { formatFileSize } from '@/utils'

/** Each target retains its own byte counters and Engine-confirmed outcome. */
export function HistoryKitFileTargetProgress({
  item,
  fileName,
  deviceName,
}: {
  item: OutboundProgressItemDto
  fileName: string
  deviceName: string
}) {
  const { t } = useTranslation()
  const percent =
    item.completedBytes != null && item.totalBytes != null && item.totalBytes > 0
      ? Math.min(100, Math.max(0, (item.completedBytes / item.totalBytes) * 100))
      : undefined
  const running = ['waiting', 'transferring', 'processing', 'retrying'].includes(item.state)
  const reason = item.failureReason
    ? t(`delivery.outbound.reason.${item.failureReason}`)
    : item.cancellationReason
      ? t(`delivery.outbound.cancel.${item.cancellationReason}`)
      : null
  return (
    <div data-file-target-state={item.state} className="flex min-w-0 flex-col gap-1.5">
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <Text role="caption">{deviceName}</Text>
        <Text role="caption" tone={item.state === 'failed' ? 'critical' : 'secondary'}>
          {t(`delivery.outbound.itemState.${item.state}`)}
          {percent !== undefined && ` · ${Math.round(percent)}%`}
        </Text>
      </div>
      {(percent !== undefined || running) && (
        <ProgressBar
          label={t('delivery.outbound.itemProgressAria', { device: deviceName, file: fileName })}
          value={percent}
        />
      )}
      <Text role="caption" tone="secondary">
        {item.completedBytes != null
          ? formatFileSize(item.completedBytes)
          : t('delivery.outbound.unknownBytes')}
        {' / '}
        {item.totalBytes != null
          ? formatFileSize(item.totalBytes)
          : t('delivery.outbound.unknownTotal')}
      </Text>
      {reason && (
        <Text role="caption" tone="secondary">
          {reason}
        </Text>
      )}
    </div>
  )
}
