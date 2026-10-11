import { useTranslation } from 'react-i18next'
import type {
  EntryDeliveryStatusView,
  EntryDeliveryView,
} from '@/api/tauri-command/clipboard_delivery'
import { deviceLabel, getStatusLabel } from '@/components/clipboard/entry-delivery-labels'
import { useResendAction } from '@/hooks/useResendAction'
import { InsetList, Text, type InsetListRow, type StatusTone } from '@/ui-kit'

const delivered = (status: EntryDeliveryStatusView) =>
  status.tag === 'delivered' || status.tag === 'duplicate'

function tone(status: EntryDeliveryStatusView): StatusTone {
  if (delivered(status)) return 'positive'
  if (status.tag === 'failed') return 'critical'
  return 'neutral'
}

/**
 * Delivery of the entry to each paired device (design MA2 "Delivery"), with the Engine's real
 * per-device states; a device that has not received it can be sent it again. The design's "In
 * clipboard" and "In history" are not states the Engine reports, so the Engine's own wording is used.
 */
export function HistoryKitDelivery({ delivery }: { delivery: EntryDeliveryView }) {
  const { t } = useTranslation()
  const action = useResendAction()
  const { source, deliveries, entryId } = delivery
  const resendable = source.tag === 'local'
  const done = deliveries.filter(target => delivered(target.status)).length
  const note =
    source.tag === 'historical'
      ? t('delivery.list.historical')
      : deliveries.length === 0
        ? t('delivery.list.noPeers')
        : null
  const rows: InsetListRow[] = deliveries.map(target => {
    const name = deviceLabel(target.targetDeviceName, target.targetDeviceId)
    const busy =
      action.isEntryInFlight(entryId) || action.isPeerInFlight(entryId, target.targetDeviceId)
    return {
      id: target.targetDeviceId,
      title: name,
      detail: getStatusLabel(target.status, t),
      tone: tone(target.status),
      action:
        resendable && !delivered(target.status) && !busy
          ? {
              label: t('delivery.resend.button.peer'),
              onClick: () => void action.resendToPeer(entryId, target.targetDeviceId),
            }
          : undefined,
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
