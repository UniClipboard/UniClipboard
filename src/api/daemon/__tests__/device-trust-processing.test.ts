import { afterEach, expect, it, vi } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { getDeviceTrustSnapshot } from '@/api/daemon/device-trust'

afterEach(() => vi.restoreAllMocks())

it('preserves a processing join and its maintenance result from the public response', async () => {
  const currentJoin = {
    status: 'processing',
    joinId: 'join-1',
    targetSpaceId: 'space-1',
    sponsorDeviceId: 'sponsor-1',
    sponsorIdentityFingerprint: 'fingerprint-1',
    peerUpgradeRequired: false,
  }
  const maintenanceHealth = {
    phase: 'retrying',
    reason: null,
    recovery: null,
    nextRetryAtMs: 12345,
  }
  vi.spyOn(daemonClient, 'callEnveloped').mockResolvedValue({
    revision: 1,
    deviceTrust: { currentJoin, maintenanceHealth },
    issues: [],
  } as never)

  const snapshot = await getDeviceTrustSnapshot()
  expect(snapshot.currentJoin).toEqual(currentJoin)
  expect(snapshot.maintenanceHealth).toEqual(maintenanceHealth)
})

it('preserves attention and unfinished pairing states from the public response', async () => {
  const currentJoin = {
    status: 'needs_attention',
    joinId: 'join-2',
    reason: 'outcome_cannot_be_proven',
    recovery: 'preserve_data_and_contact_support',
    nextRetryAtMs: null,
  }
  const inboundPairings = [
    {
      pairingId: 'pairing-1',
      deviceId: null,
      displayName: null,
      status: 'needs_attention',
    },
  ]
  vi.spyOn(daemonClient, 'callEnveloped').mockResolvedValue({
    revision: 2,
    deviceTrust: { currentJoin, inboundPairings },
    issues: [],
  } as never)

  const snapshot = await getDeviceTrustSnapshot()
  expect(snapshot.currentJoin).toEqual(currentJoin)
  expect(snapshot.inboundPairings).toEqual(inboundPairings)
})
