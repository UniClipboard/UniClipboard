import type { DeviceTrustRelationship, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import type { SpaceMember } from '@/api/daemon/members'
import type { StatusDotTone } from '@/components/device/StatusDot'

export type DeviceRowStatus = {
  kind:
    | 'paused'
    | 'unknown'
    | 'recently_active'
    | 'online'
    | 'offline'
    | 'waiting_for_update'
    | 'removing'
    | 'recovery_required'
    | 'removed'
    | 'removal_notification_pending'
    | 'diverged'
    | 'security_update'
  label: string
  description?: string
  detail?: string
  /** Status tone for detail views; rows receive it alongside the status. */
  tone?: StatusDotTone
}

export interface DeviceTrustListView {
  peers: SpaceMember[]
  relationshipsByDeviceId: Map<string, DeviceTrustRelationship>
  localRelationship: DeviceTrustRelationship | null
  removedDevices: DeviceTrustRelationship[]
}

export function buildDeviceTrustListView(
  admittedPeers: SpaceMember[],
  snapshot: DeviceTrustSnapshot | null
): DeviceTrustListView {
  const relationshipsByDeviceId = new Map(
    (snapshot?.devices ?? []).map(device => [device.deviceId, device])
  )

  return {
    peers: admittedPeers,
    relationshipsByDeviceId,
    localRelationship: snapshot?.devices.find(device => device.isLocal) ?? null,
    removedDevices: (snapshot?.devices ?? []).filter(
      device =>
        !device.isLocal &&
        device.membership === 'removed' &&
        device.groupRelationship === 'awaiting_removal_acknowledgement'
    ),
  }
}

export function getDeviceTrustStatus(
  device: DeviceTrustRelationship,
  t: (key: string, opts?: Record<string, unknown>) => string
): { tone: StatusDotTone; status: DeviceRowStatus } | null {
  if (device.groupRelationship === 'awaiting_removal_acknowledgement') {
    return {
      tone: 'info',
      status: {
        kind: 'removal_notification_pending',
        label: t('devices.memberRemoval.notificationPending.title'),
        description: t('devices.memberRemoval.notificationPending.description'),
      },
    }
  }
  if (device.groupRelationship === 'diverged') {
    return {
      tone: 'warning',
      status: { kind: 'diverged', label: t('deviceTrust.status.diverged') },
    }
  }
  if (device.groupRelationship === 'unverifiable') {
    return {
      tone: 'warning',
      status: { kind: 'recovery_required', label: t('deviceTrust.status.unverifiable') },
    }
  }
  if (device.compatibility === 'upgrade_required') {
    return {
      tone: 'warning',
      status: { kind: 'waiting_for_update', label: t('deviceTrust.status.upgrade') },
    }
  }
  if (device.groupRelationship === 'pending_local_decision') {
    return { tone: 'warning', status: { kind: 'removing', label: t('deviceTrust.status.pending') } }
  }
  if (device.groupUpdate) return getGroupUpdateStatus(device.groupUpdate, t)
  return null
}

/**
 * Presents the Engine-owned delivery state of this device's oldest unacknowledged
 * group key update. The Engine alone advances or settles it; the description only
 * explains who acts next and never offers a host-side retry.
 */
function getGroupUpdateStatus(
  update: NonNullable<DeviceTrustRelationship['groupUpdate']>,
  t: (key: string, opts?: Record<string, unknown>) => string
): { tone: StatusDotTone; status: DeviceRowStatus } {
  const rejected =
    update.state === 'rejected_unclassified' || update.state === 'rejected_by_receiver'
  const description =
    // A deadline that has passed (0 for a first attempt) means the retry is already due.
    update.recovery === 'automatic' &&
    update.nextRetryAtMs != null &&
    update.nextRetryAtMs > Date.now()
      ? t('deviceTrust.groupUpdate.recovery.automaticAt', {
          time: formatRetryTime(update.nextRetryAtMs),
        })
      : t(`deviceTrust.groupUpdate.recovery.${update.recovery}`)
  const tone: StatusDotTone = rejected ? 'warning' : 'info'
  return {
    tone,
    status: {
      kind: 'security_update',
      tone,
      label: t(`deviceTrust.groupUpdate.state.${update.state}`),
      description,
      detail: t('deviceTrust.groupUpdate.detail', {
        epoch: update.epoch,
        attempts: update.attempts,
      }),
    },
  }
}

function formatRetryTime(ms: number): string {
  const at = new Date(ms)
  const sameDay = at.toDateString() === new Date().toDateString()
  return at.toLocaleString([], {
    ...(sameDay ? {} : { month: 'short', day: 'numeric' }),
    hour: '2-digit',
    minute: '2-digit',
  })
}
