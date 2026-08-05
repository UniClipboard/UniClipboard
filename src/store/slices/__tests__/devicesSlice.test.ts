import { configureStore } from '@reduxjs/toolkit'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  getCurrentMemberRemoval,
  getMembershipConvergence,
  getSpaceProtection,
  type MemberRemoval,
  type MembershipConvergence,
  type SpaceProtection,
} from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import { getNetworkRecoveryStatus, type NetworkRecoveryStatus } from '@/api/daemon/network-recovery'
import devicesReducer, {
  fetchCurrentMemberRemoval,
  fetchMembershipConvergence,
  fetchNetworkRecoveryStatus,
  fetchSpaceProtection,
  setMemberRemoval,
  setSpaceMembers,
} from '../devicesSlice'

vi.mock('@/api/daemon/member', async importOriginal => ({
  ...(await importOriginal<typeof import('@/api/daemon/member')>()),
  getCurrentMemberRemoval: vi.fn(),
  getMembershipConvergence: vi.fn(),
  getSpaceProtection: vi.fn(),
}))

vi.mock('@/api/daemon/network-recovery', () => ({
  getNetworkRecoveryStatus: vi.fn(),
}))

function makeMember(peerId: string, overrides?: Partial<SpaceMember>): SpaceMember {
  return {
    peerId,
    deviceName: `device-${peerId}`,
    pairingState: 'Trusted',
    lastSeenAtMs: null,
    connected: true,
    channel: 'direct',
    connectionAddress: '192.168.1.2:5000',
    ...overrides,
  }
}

function stateWith(members: SpaceMember[]) {
  return devicesReducer(undefined, setSpaceMembers(members))
}

describe('devicesSlice setSpaceMembers', () => {
  it('replaces the member list and clears loading/error', () => {
    const next = stateWith([makeMember('a'), makeMember('b')])
    expect(next.spaceMembers.map(m => m.peerId)).toEqual(['a', 'b'])
    expect(next.spaceMembersLoading).toBe(false)
    expect(next.spaceMembersError).toBeNull()
  })

  it('reuses the previous object identity for unchanged peers', () => {
    const first = stateWith([makeMember('a'), makeMember('b')])
    const aRef = first.spaceMembers.find(m => m.peerId === 'a')

    // 'a' unchanged, 'b' flips connected → only 'b' should get a new identity.
    const second = devicesReducer(
      first,
      setSpaceMembers([makeMember('a'), makeMember('b', { connected: false })])
    )

    expect(second.spaceMembers.find(m => m.peerId === 'a')).toBe(aRef)
    expect(second.spaceMembers.find(m => m.peerId === 'b')?.connected).toBe(false)
  })

  it('adds new peers and drops removed ones', () => {
    const first = stateWith([makeMember('a'), makeMember('b')])
    const second = devicesReducer(first, setSpaceMembers([makeMember('a'), makeMember('c')]))
    expect(second.spaceMembers.map(m => m.peerId).sort()).toEqual(['a', 'c'])
  })
})

describe('devicesSlice fetchSpaceProtection', () => {
  const protection: SpaceProtection = {
    mode: 'migrating',
    members: [{ deviceId: 'peer-a', status: 'awaiting_readmission' }],
    legacyBootstrap: {
      bootstrapId: 'bootstrap-1',
      outcome: 'awaiting_readmission',
      pendingReadmission: 1,
    },
  }

  beforeEach(() => {
    vi.mocked(getSpaceProtection).mockReset()
  })

  it('stores the Engine-authoritative protection snapshot', async () => {
    vi.mocked(getSpaceProtection).mockResolvedValue(protection)
    const store = configureStore({ reducer: { devices: devicesReducer } })

    await store.dispatch(fetchSpaceProtection())

    expect(store.getState().devices.spaceProtection).toEqual(protection)
    expect(store.getState().devices.spaceProtectionError).toBeNull()
  })

  it('stores a translation key when the status request fails', async () => {
    vi.mocked(getSpaceProtection).mockRejectedValue(new Error('offline'))
    const store = configureStore({ reducer: { devices: devicesReducer } })

    await store.dispatch(fetchSpaceProtection())

    expect(store.getState().devices.spaceProtectionError).toBe(
      'devices.protection.errors.statusFailed'
    )
  })
})

describe('devicesSlice fetchMembershipConvergence', () => {
  const convergence: MembershipConvergence = { state: 'converging' }

  beforeEach(() => {
    vi.mocked(getMembershipConvergence).mockReset()
  })

  it('stores the Engine-authoritative space connection state', async () => {
    vi.mocked(getMembershipConvergence).mockResolvedValue(convergence)
    const store = configureStore({ reducer: { devices: devicesReducer } })

    await store.dispatch(fetchMembershipConvergence())

    expect(store.getState().devices.membershipConvergence).toEqual(convergence)
    expect(store.getState().devices.membershipConvergenceError).toBeNull()
  })

  it('keeps the last status when a refresh fails', async () => {
    vi.mocked(getMembershipConvergence)
      .mockResolvedValueOnce(convergence)
      .mockRejectedValueOnce(new Error('offline'))
    const store = configureStore({ reducer: { devices: devicesReducer } })

    await store.dispatch(fetchMembershipConvergence())
    await store.dispatch(fetchMembershipConvergence())

    expect(store.getState().devices.membershipConvergence).toEqual(convergence)
    expect(store.getState().devices.membershipConvergenceError).toBe(
      'Failed to fetch membership convergence'
    )
  })
})

describe('devicesSlice fetchNetworkRecoveryStatus', () => {
  const failed: NetworkRecoveryStatus = {
    phase: 'failed',
    retryable: true,
  }

  beforeEach(() => {
    vi.mocked(getNetworkRecoveryStatus).mockReset()
  })

  it('stores the Engine-authoritative recovery snapshot', async () => {
    vi.mocked(getNetworkRecoveryStatus).mockResolvedValue(failed)
    const store = configureStore({ reducer: { devices: devicesReducer } })

    await store.dispatch(fetchNetworkRecoveryStatus())

    expect(store.getState().devices.networkRecovery).toEqual(failed)
    expect(store.getState().devices.networkRecoveryError).toBeNull()
  })

  it('keeps the last recovery snapshot when the status read fails', async () => {
    vi.mocked(getNetworkRecoveryStatus)
      .mockResolvedValueOnce(failed)
      .mockRejectedValueOnce(new Error('offline'))
    const store = configureStore({ reducer: { devices: devicesReducer } })

    await store.dispatch(fetchNetworkRecoveryStatus())
    await store.dispatch(fetchNetworkRecoveryStatus())

    expect(store.getState().devices.networkRecovery).toEqual(failed)
    expect(store.getState().devices.networkRecoveryError).toBe(
      'devices.networkRecovery.errors.statusFailed'
    )
  })
})

describe('devicesSlice fetchCurrentMemberRemoval', () => {
  const removal: MemberRemoval = {
    revocationId: 'removal-1',
    outcome: 'applied',
    pendingRecipients: 1,
    removedDeviceIds: ['removed-device'],
    pendingRecipientDeviceIds: ['retained-device'],
    updatedAtMs: 1_700_000_000_000,
  }

  beforeEach(() => {
    vi.mocked(getCurrentMemberRemoval).mockReset()
  })

  it('keeps a newer removal snapshot when an older refresh returns null', async () => {
    let resolveRefresh: ((value: MemberRemoval | null) => void) | undefined
    vi.mocked(getCurrentMemberRemoval).mockImplementation(
      () => new Promise<MemberRemoval | null>(resolve => (resolveRefresh = resolve))
    )
    const store = configureStore({ reducer: { devices: devicesReducer } })

    const refresh = store.dispatch(fetchCurrentMemberRemoval())
    store.dispatch(setMemberRemoval(removal))
    resolveRefresh?.(null)
    await refresh

    expect(store.getState().devices.memberRemoval).toEqual(removal)
    expect(store.getState().devices.memberRemovalError).toBeNull()
  })
})
