import type { DeviceTrustRelationship } from '@/api/daemon/device-trust'

export function activeDeviceIds(snapshot: {
  devices: ReadonlyArray<Pick<DeviceTrustRelationship, 'deviceId' | 'membership'>>
}): ReadonlySet<string> {
  const deviceIds = new Set<string>()
  for (const device of snapshot.devices) {
    if (device.membership === 'active') deviceIds.add(device.deviceId)
  }
  return deviceIds
}

export function findNewActiveDeviceId(
  previousDeviceIds: ReadonlySet<string>,
  currentDeviceIds: ReadonlySet<string>
): string | null {
  for (const deviceId of currentDeviceIds) {
    if (!previousDeviceIds.has(deviceId)) return deviceId
  }
  return null
}
