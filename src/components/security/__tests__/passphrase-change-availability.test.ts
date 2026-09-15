import { describe, expect, it } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { getPassphraseChangeAvailability } from '@/components/security/passphrase-change-availability'

function snapshot(
  peers: Array<{ deviceId: string; membership: 'active' | 'removed' | 'unavailable' | 'unknown' }>
): DeviceTrustSnapshot {
  return {
    revision: 1,
    localDeviceId: 'local',
    localMembership: 'active',
    currentChange: null,
    currentJoin: null,
    pendingInboundMember: null,
    devices: [
      {
        deviceId: 'local',
        displayName: 'This device',
        isLocal: true,
        reachability: 'online',
        membership: 'active',
        groupRelationship: 'consistent',
        compatibility: 'compatible',
        syncRelationship: 'usable',
        availableActions: [],
        blockedReason: null,
      },
      ...peers.map(peer => ({
        ...peer,
        displayName: peer.deviceId,
        isLocal: false,
        reachability: 'offline' as const,
        groupRelationship: 'consistent' as const,
        compatibility: 'compatible' as const,
        syncRelationship: 'usable' as const,
        availableActions: [],
        blockedReason: null,
      })),
    ],
    recovery: 'not_available_in_this_version',
    allowedActions: [],
    blockedReason: null,
    updatedAtMs: 1,
  }
}

describe('getPassphraseChangeAvailability', () => {
  it('allows a space containing only the local device', () => {
    expect(getPassphraseChangeAvailability(snapshot([]), false)).toBe('available')
  })

  it.each(['active', 'unavailable', 'unknown'] as const)(
    'blocks when another device is %s',
    membership => {
      expect(
        getPassphraseChangeAvailability(snapshot([{ deviceId: 'peer', membership }]), false)
      ).toBe('multiple_devices')
    }
  )

  it('ignores devices already removed from the space', () => {
    expect(
      getPassphraseChangeAvailability(
        snapshot([{ deviceId: 'removed', membership: 'removed' }]),
        false
      )
    ).toBe('available')
  })

  it('fails closed while device state is missing', () => {
    expect(getPassphraseChangeAvailability(null, true)).toBe('checking')
    expect(getPassphraseChangeAvailability(null, false)).toBe('unavailable')
  })
})
