import { useTranslation } from 'react-i18next'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import { Disclosure } from '@/ui-kit'
import { HistoryKitFileTargetProgress } from './HistoryKitFileTargetProgress'

/** File indices identify files; representation and directory indices are separate namespaces. */
export function HistoryKitFileOutbound({
  index,
  fileName,
  snapshots,
  deviceNames,
}: {
  index: number
  fileName: string
  snapshots: OutboundProgressSnapshot[]
  deviceNames: Record<string, string>
}) {
  const { t } = useTranslation()
  const targets = snapshots.flatMap(snapshot => {
    const item = snapshot.items?.find(item => item.kind === 'file' && item.index === index)
    return item
      ? [
          {
            deviceId: snapshot.targetDeviceId,
            item: snapshot.freshness === 'unconfirmed' ? { ...item, state: 'unconfirmed' } : item,
          },
        ]
      : []
  })
  if (targets.length === 0) return null
  const completed = targets.filter(target => target.item.state === 'completed').length
  const failed = targets.filter(target => target.item.state === 'failed').length
  const label = [
    t('delivery.outbound.fileTargets', { completed, total: targets.length }),
    failed > 0 ? t('delivery.outbound.failedTargets', { count: failed }) : null,
  ]
    .filter(Boolean)
    .join(' · ')
  const content = (
    <div className="flex min-w-0 flex-col gap-3" data-outbound-file={index}>
      {targets.map(target => (
        <HistoryKitFileTargetProgress
          key={target.deviceId}
          item={target.item}
          fileName={fileName}
          deviceName={deviceNames[target.deviceId] ?? target.deviceId.slice(0, 8)}
        />
      ))}
    </div>
  )
  return targets.length === 1 ? (
    content
  ) : (
    <Disclosure label={label} defaultOpen={completed !== targets.length}>
      {content}
    </Disclosure>
  )
}
