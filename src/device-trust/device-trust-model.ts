import type {
  DeviceTrustImpact,
  DeviceTrustRelationship,
  DeviceTrustSnapshot,
} from '@/api/daemon/device-trust'

export interface DeviceTrustDisplayDevice {
  deviceId: string
  label: string
  relationship: DeviceTrustRelationship | null
}

export interface DeviceTrustImpactView {
  usable: DeviceTrustDisplayDevice[]
  paused: DeviceTrustDisplayDevice[]
  requiresRejoin: DeviceTrustDisplayDevice[]
}

export interface PendingDeviceTrustDecisionView {
  proposer: DeviceTrustDisplayDevice
  targets: DeviceTrustDisplayDevice[]
  includesLocalDevice: boolean
  apply: DeviceTrustImpactView
  keepCurrent: DeviceTrustImpactView
}

export function getDeviceLabel(displayName: string, deviceId: string): string {
  const name = displayName.trim() || 'Unnamed device'
  const suffix = deviceId.slice(-4).toUpperCase()
  return suffix ? `${name} · ${suffix}` : name
}

export function getPendingDecisionView(
  snapshot: DeviceTrustSnapshot
): PendingDeviceTrustDecisionView | null {
  const change = snapshot.currentChange
  if (!change) return null

  const byId = new Map(snapshot.devices.map(device => [device.deviceId, device]))
  const display = (deviceId: string): DeviceTrustDisplayDevice => {
    const relationship = byId.get(deviceId) ?? null
    return {
      deviceId,
      label: getDeviceLabel(relationship?.displayName ?? '', deviceId),
      relationship,
    }
  }
  const impact = (value: DeviceTrustImpact): DeviceTrustImpactView => ({
    usable: value.usableDeviceIds.map(display),
    paused: value.pausedDeviceIds.map(display),
    requiresRejoin: value.requiresRejoinDeviceIds.map(display),
  })

  return {
    proposer: display(change.proposedByDeviceId),
    targets: change.targetDeviceIds.map(display),
    includesLocalDevice: change.includesLocalDevice,
    apply: impact(change.applyImpact),
    keepCurrent: impact(change.keepCurrentImpact),
  }
}
