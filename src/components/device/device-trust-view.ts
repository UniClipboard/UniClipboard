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
    | 'diverged'
  label: string
  description?: string
}

export interface DeviceTrustListView {
  peers: SpaceMember[]
  relationshipsByDeviceId: Map<string, DeviceTrustRelationship>
  localRelationship: DeviceTrustRelationship | null
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
  }
}

export function getDeviceTrustStatus(
  device: DeviceTrustRelationship,
  t: (key: string) => string
): { tone: StatusDotTone; status: DeviceRowStatus } | null {
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
  return null
}
