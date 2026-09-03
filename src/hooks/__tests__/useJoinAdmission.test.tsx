import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useJoinAdmission } from '@/hooks/useJoinAdmission'

const getDeviceTrustSnapshot = vi.hoisted(() => vi.fn())
const subscribe = vi.hoisted(() => vi.fn())
const onReconnect = vi.hoisted(() => vi.fn(() => () => undefined))

vi.mock('@/api/daemon/device-trust', () => ({
  getDeviceTrustSnapshot: () => getDeviceTrustSnapshot(),
}))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: { subscribe, onReconnect },
}))

describe('useJoinAdmission', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    subscribe.mockImplementation((_topics, _callback) => () => undefined)
    getDeviceTrustSnapshot.mockResolvedValue({ currentJoin: null })
  })

  it('rechecks a pending join after a global refresh notification', async () => {
    let handler: ((event: { eventType: string }) => void) | undefined
    subscribe.mockImplementation((_topics, callback) => {
      handler = callback
      return () => undefined
    })
    const resolved = vi.fn()
    getDeviceTrustSnapshot.mockResolvedValueOnce({ currentJoin: null }).mockResolvedValueOnce({
      currentJoin: {
        status: 'active',
        joinId: 'join-1',
        joinedSpace: {
          sponsorDeviceId: 'sponsor',
          sponsorIdentityFingerprint: 'sponsor-fingerprint',
          spaceId: 'space-1',
          selfDeviceId: 'local',
          selfIdentityFingerprint: 'local-fingerprint',
          migratedRecords: null,
          preservedUnreadableRecords: null,
        },
      },
    })

    renderHook(() => useJoinAdmission('join-1', new Set(), resolved))
    await waitFor(() => expect(getDeviceTrustSnapshot).toHaveBeenCalledTimes(1))
    expect(subscribe).toHaveBeenCalledWith(['device-trust', 'system'], expect.any(Function))

    await act(async () => handler?.({ eventType: 'system.refresh_required' }))

    await waitFor(() =>
      expect(resolved).toHaveBeenCalledWith({
        status: 'active',
        peerDeviceId: 'sponsor',
        result: expect.objectContaining({ status: 'active', joinId: 'join-1' }),
      })
    )
  })

  it('resolves from a newly active member when the completed join record is no longer present', async () => {
    let handler: ((event: { eventType: string }) => void) | undefined
    subscribe.mockImplementation((_topics, callback) => {
      handler = callback
      return () => undefined
    })
    const resolved = vi.fn()
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({ currentJoin: null, localMembership: 'unavailable', devices: [] })
      .mockResolvedValueOnce({
        currentJoin: null,
        localMembership: 'active',
        devices: [
          { deviceId: 'local', isLocal: true, membership: 'active' },
          { deviceId: 'sponsor', isLocal: false, membership: 'active' },
        ],
      })

    renderHook(() => useJoinAdmission('join-1', new Set(), resolved))
    await waitFor(() => expect(getDeviceTrustSnapshot).toHaveBeenCalledTimes(1))
    await act(async () => handler?.({ eventType: 'device-trust.changed' }))

    await waitFor(() =>
      expect(resolved).toHaveBeenCalledWith({
        status: 'active',
        peerDeviceId: 'sponsor',
        result: null,
      })
    )
  })
})
