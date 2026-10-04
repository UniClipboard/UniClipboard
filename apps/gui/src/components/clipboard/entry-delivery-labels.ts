import type {
  DeliveryFailureReason,
  EntryDeliveryStatusView,
} from '@/api/tauri-command/clipboard_delivery'

const FAILURE_REASON_KEYS: Record<DeliveryFailureReason, string> = {
  localPolicy: 'delivery.failureReason.localPolicy',
  peerRejected: 'delivery.failureReason.peerRejected',
  peerIncompatible: 'delivery.failureReason.peerIncompatible',
  io: 'delivery.failureReason.io',
  internal: 'delivery.failureReason.internal',
}

function truncateDeviceId(deviceId: string): string {
  if (deviceId.length <= 10) return deviceId
  return `${deviceId.slice(0, 8)}…`
}

/** Prefer the resolved device name; fall back to a truncated device id. */
export function deviceLabel(name: string | null | undefined, deviceId: string): string {
  if (name && name.trim().length > 0) return name
  return truncateDeviceId(deviceId)
}

export function getStatusLabel(
  status: EntryDeliveryStatusView,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  switch (status.tag) {
    case 'delivered':
      return t('delivery.status.delivered')
    case 'duplicate':
      return t('delivery.status.duplicate')
    case 'pending':
      return t('delivery.status.pending')
    case 'unreachable':
      return t('delivery.status.unreachable')
    case 'superseded':
      return t('delivery.status.superseded')
    case 'failed':
      return t('delivery.status.failedWithReason', {
        reason: t(FAILURE_REASON_KEYS[status.reason]),
      })
  }
}

export interface StatusTone {
  icon: string
  label: string
}

export function renderStatusTone(status: EntryDeliveryStatusView): StatusTone {
  switch (status.tag) {
    case 'delivered':
      return { icon: 'text-emerald-500', label: 'text-foreground/80' }
    case 'duplicate':
      return { icon: 'text-emerald-500/70', label: 'text-muted-foreground' }
    case 'pending':
      return { icon: 'text-muted-foreground/60', label: 'text-muted-foreground' }
    case 'unreachable':
      return { icon: 'text-muted-foreground/60', label: 'text-muted-foreground' }
    case 'superseded':
      return { icon: 'text-muted-foreground/60', label: 'text-muted-foreground' }
    case 'failed':
      return { icon: 'text-destructive', label: 'text-destructive' }
  }
}
