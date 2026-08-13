import type { DeviceTrustRelationship, DeviceTrustSnapshot } from '@/api/daemon/device-trust'

export interface DeviceTrustDisplayDevice {
  deviceId: string
  label: string
  relationship: DeviceTrustRelationship | null
}

export interface DeviceTrustChoiceView {
  continuesWith: DeviceTrustDisplayDevice[]
  stopsWith: DeviceTrustDisplayDevice[]
}

export interface PendingDeviceTrustDecisionView {
  proposer: DeviceTrustDisplayDevice
  targets: DeviceTrustDisplayDevice[]
  includesLocalDevice: boolean
  apply: DeviceTrustChoiceView
  keepCurrent: DeviceTrustChoiceView
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
  const nameCounts = new Map<string, number>()
  for (const device of snapshot.devices) {
    const name = device.displayName.trim().toLocaleLowerCase()
    if (name) nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1)
  }
  const display = (deviceId: string): DeviceTrustDisplayDevice => {
    const relationship = byId.get(deviceId) ?? null
    const displayName = relationship?.displayName.trim() ?? ''
    const hasDuplicateName = displayName
      ? (nameCounts.get(displayName.toLocaleLowerCase()) ?? 0) > 1
      : false
    return {
      deviceId,
      label: displayName && !hasDuplicateName ? displayName : getDeviceLabel(displayName, deviceId),
      relationship,
    }
  }
  const otherDevices = (deviceIds: string[]) =>
    deviceIds.flatMap(deviceId => (deviceId === snapshot.localDeviceId ? [] : [display(deviceId)]))
  const localDeviceWillBeRemoved = change.applyImpact.localDeviceOutcome === 'removed'

  return {
    proposer: display(change.proposedByDeviceId),
    targets: change.targetDeviceIds.map(display),
    includesLocalDevice: change.includesLocalDevice,
    apply: {
      continuesWith: localDeviceWillBeRemoved
        ? []
        : otherDevices(change.applyImpact.usableDeviceIds),
      stopsWith: localDeviceWillBeRemoved
        ? otherDevices(change.applyImpact.usableDeviceIds)
        : otherDevices(change.targetDeviceIds),
    },
    keepCurrent: {
      continuesWith: otherDevices(change.keepCurrentImpact.usableDeviceIds),
      stopsWith: otherDevices(change.keepCurrentImpact.pausedDeviceIds),
    },
  }
}
