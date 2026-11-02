import { useTranslation } from 'react-i18next'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import { deviceLabel } from '@/components/clipboard/entry-delivery-labels'
import { useResendAction } from '@/hooks/useResendAction'
import { deliveryStatus } from '@/pages/history-kit/delivery-status'
import { HistoryKitDeliveryStatus } from '@/pages/history-kit/HistoryKitDeliveryStatus'
import { useAppSelector } from '@/store/hooks'
import { selectOutboundByEntry } from '@/store/slices/outboundProgressSlice'
import { InsetList, Text, type InsetListRow } from '@/ui-kit'

/**
 * Delivery of the entry to each paired device (design MA2 "Delivery"), with the Engine's real
 * per-device states; a device that has not received it can be sent it again. The design's "In
 * clipboard" and "In history" are not states the Engine reports, so the Engine's own wording is used.
 */
export function HistoryKitDelivery({
  delivery,
  fileNames,
}: {
  delivery: EntryDeliveryView
  fileNames?: readonly string[]
}) {
  const { t } = useTranslation()
  const action = useResendAction()
  const { source, deliveries, entryId } = delivery
  const resendable = source.tag === 'local'
  const snapshots = useAppSelector(state => selectOutboundByEntry(state, entryId))
  const targets = deliveries.map(target => {
    const snapshot = snapshots?.[target.targetDeviceId]
    const busy =
      action.isEntryInFlight(entryId) || action.isPeerInFlight(entryId, target.targetDeviceId)
    return { target, snapshot, view: deliveryStatus(target.status, snapshot, busy) }
  })
  const done = targets.filter(({ view }) => view.complete).length
  const note =
    source.tag === 'historical'
      ? t('delivery.list.historical')
      : deliveries.length === 0
        ? t('delivery.list.noPeers')
        : null
  const rows: InsetListRow[] = targets.map(({ target, snapshot, view }) => {
    const name = deviceLabel(target.targetDeviceName, target.targetDeviceId)
    return {
      id: target.targetDeviceId,
      title: name,
      tone: view.tone,
      status: (
        <HistoryKitDeliveryStatus
          entryId={entryId}
          target={target}
          snapshot={snapshot}
          resendable={resendable}
          fileNames={fileNames}
        />
      ),
    }
  })
  return (
    <section className="flex shrink-0 flex-col gap-1.5" aria-label={t('delivery.section.aria')}>
      <div className="flex items-baseline gap-2">
        <Text role="headline">{t('history.kit.delivery')}</Text>
        {deliveries.length > 0 && (
          <Text role="caption" tone="secondary">
            {t('history.kit.deliveredCount', { done, total: deliveries.length })}
          </Text>
        )}
      </div>
      {note ? (
        <Text role="caption" tone="secondary">
          {note}
        </Text>
      ) : (
        <InsetList label={t('delivery.list.title')} rows={rows} />
      )}
      <Text role="caption" tone="tertiary">
        {t('history.kit.encryptionNote')}
      </Text>
    </section>
  )
}
