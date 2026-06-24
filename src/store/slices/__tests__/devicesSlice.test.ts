import { describe, it, expect } from 'vitest'
import type { SpaceMember } from '@/api/daemon/members'
import devicesReducer, { setSpaceMembers } from '../devicesSlice'

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
