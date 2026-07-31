import { configureStore } from '@reduxjs/toolkit'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getSpaceProtection, type SpaceProtection } from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import devicesReducer, { fetchSpaceProtection, setSpaceMembers } from '../devicesSlice'

vi.mock('@/api/daemon/member', async importOriginal => ({
  ...(await importOriginal<typeof import('@/api/daemon/member')>()),
  getSpaceProtection: vi.fn(),
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
