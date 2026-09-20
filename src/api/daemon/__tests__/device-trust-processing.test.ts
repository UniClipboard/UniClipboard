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
