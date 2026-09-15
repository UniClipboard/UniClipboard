import { describe, expect, it } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import type { SpaceMember } from '@/api/daemon/members'
import {
  buildDeviceTrustListView,
  getDeviceTrustStatus,
} from '@/components/device/device-trust-view'

const admittedPeer: SpaceMember = {
  peerId: 'peer-a',
  deviceName: 'Peer A',
  pairingState: 'trusted',
  lastSeenAtMs: null,
  connected: true,
  channel: 'direct',
  connectionAddress: null,
}

const snapshot: DeviceTrustSnapshot = {
  revision: 1,
  localDeviceId: 'local',
  localMembership: 'active',
  currentChange: null,
  devices: [
    {
      deviceId: 'local',
      displayName: 'Local',
      isLocal: true,
      reachability: 'online',
      membership: 'active',
      groupRelationship: 'consistent',
      compatibility: 'compatible',
      syncRelationship: 'usable',
      availableActions: [],
      blockedReason: null,
    },
    {
      deviceId: 'peer-a',
      displayName: 'Peer A',
      isLocal: false,
      reachability: 'online',
      membership: 'active',
      groupRelationship: 'consistent',
      compatibility: 'compatible',
      syncRelationship: 'usable',
      availableActions: [],
      blockedReason: null,
    },
    {
      deviceId: 'peer-b',
      displayName: 'Peer B',
      isLocal: false,
      reachability: 'offline',
      membership: 'active',
      groupRelationship: 'diverged',
      compatibility: 'compatible',
      syncRelationship: 'paused_group_diverged',
      availableActions: [],
      blockedReason: null,
    },
  ],
  recovery: 'not_available_in_this_version',
  allowedActions: [],
  blockedReason: null,
  updatedAtMs: 1,
}

describe('device trust list view', () => {
  it.each([
    [
      'awaiting_peer_confirmation',
      'pairing_awaiting_confirmation',
      'deviceTrust.status.awaitingPeerConfirmation',
    ],
    ['unconfirmed', 'pairing_unconfirmed', 'deviceTrust.status.unconfirmed'],
  ] as const)('shows the Engine-owned pairing state %s', (pairingConfirmation, kind, label) => {
    expect(
      getDeviceTrustStatus(
        {
          ...snapshot.devices[1],
          groupRelationship: 'confirmation_pending',
          pairingConfirmation,
        },
        key => key
      )
    ).toEqual({ tone: 'warning', status: { kind, label } })
  })

  it('returns to normal connection status after pairing is confirmed', () => {
    expect(
      getDeviceTrustStatus(
        {
          ...snapshot.devices[1],
          pairingConfirmation: 'confirmed',
        },
        key => key
      )
    ).toBeNull()
  })

  it('indexes relationships without adding devices missing from current membership', () => {
    const view = buildDeviceTrustListView([admittedPeer], snapshot)

    expect(view.peers.map(peer => peer.peerId)).toEqual(['peer-a'])
    expect(view.relationshipsByDeviceId.get('peer-a')?.displayName).toBe('Peer A')
    expect(view.relationshipsByDeviceId.get('peer-b')?.displayName).toBe('Peer B')
    expect(view.localRelationship?.deviceId).toBe('local')
  })
})
