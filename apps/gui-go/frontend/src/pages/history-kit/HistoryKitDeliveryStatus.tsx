import { useTranslation } from 'react-i18next'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import type { EntryDeliveryTargetView } from '@/api/tauri-command/clipboard_delivery'
import { deviceLabel, getStatusLabel } from '@/components/clipboard/entry-delivery-labels'
import { useResendAction } from '@/hooks/useResendAction'
import { deliveryStatus, transferBytes, transferDetails } from '@/pages/history-kit/delivery-status'
import { TransferStatus } from '@/ui-kit'

export function HistoryKitDeliveryStatus({
  entryId,
  target,
  snapshot,
  resendable,
  fileNames,
}: {
  entryId: string
  target: EntryDeliveryTargetView
  snapshot?: OutboundProgressSnapshot
  resendable: boolean
  fileNames?: readonly string[]
}) {
  const { t } = useTranslation()
  const action = useResendAction()
  const busy =
    action.isEntryInFlight(entryId) || action.isPeerInFlight(entryId, target.targetDeviceId)
  const view = deliveryStatus(target.status, snapshot, busy)
  const name = deviceLabel(target.targetDeviceName, target.targetDeviceId)
  const label = busy
    ? t('delivery.resend.button.pending')
    : view.state
      ? t(`delivery.outbound.state.${view.state}`, {
          defaultValue: t('delivery.outbound.state.unconfirmed'),
        })
      : getStatusLabel(target.status, t)
  const retry = resendable && !view.complete && !view.running
  const { percent } = transferBytes(snapshot?.items ?? [])
  const details = transferDetails(snapshot, fileNames, t)
  return (
    <TransferStatus
      data-testid={`delivery-status-${target.targetDeviceId}`}
      label={`${name}: ${label}${retry ? ` · ${t('delivery.resend.button.peerAria', { device: name })}` : ''}`}
      title={name}
      status={label}
      tone={view.tone}
      value={busy ? undefined : (percent ?? undefined)}
      disabled={(view.failed && !retry) || busy}
      onRetry={retry ? () => void action.resendToPeer(entryId, target.targetDeviceId) : undefined}
      summary={details.summary}
      items={details.items}
    />
  )
}
