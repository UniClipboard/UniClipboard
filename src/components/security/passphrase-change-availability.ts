import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'

export type PassphraseChangeAvailability =
  | 'available'
  | 'checking'
  | 'multiple_devices'
  | 'unavailable'

export function getPassphraseChangeAvailability(
  snapshot: DeviceTrustSnapshot | null,
  loading: boolean
): PassphraseChangeAvailability {
  if (snapshot === null) return loading ? 'checking' : 'unavailable'
  if (snapshot.localMembership !== 'active') return 'unavailable'

  const hasPeer = snapshot.devices.some(
    device => device.deviceId !== snapshot.localDeviceId && device.membership !== 'removed'
  )
  return hasPeer ? 'multiple_devices' : 'available'
}

export function getPassphraseChangeAvailabilityMessageKey(
  availability: PassphraseChangeAvailability
): string {
  switch (availability) {
    case 'available':
      return 'passphraseChange.entry.description'
    case 'multiple_devices':
      return 'passphraseChange.availability.multipleDevices'
    case 'checking':
      return 'passphraseChange.availability.checking'
    case 'unavailable':
      return 'passphraseChange.availability.unavailable'
  }
}
