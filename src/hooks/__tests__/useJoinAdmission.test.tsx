import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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
  afterEach(() => vi.useRealTimers())

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

    renderHook(() => useJoinAdmission('join-1', resolved))
    await waitFor(() => expect(getDeviceTrustSnapshot).toHaveBeenCalledTimes(1))
    expect(subscribe).toHaveBeenCalledWith(['device-trust', 'system'], expect.any(Function))

    await act(async () => handler?.({ eventType: 'system.refresh_required' }))

    await waitFor(() =>
      expect(resolved).toHaveBeenCalledWith({
        status: 'active',
        joinId: 'join-1',
        joinedSpace: expect.objectContaining({ sponsorDeviceId: 'sponsor' }),
      })
    )
  })

  it('rechecks a pending join when no realtime notification arrives', async () => {
    vi.useFakeTimers()
    const resolved = vi.fn()
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({
        currentJoin: { status: 'pending', joinId: 'join-1' },
        localMembership: 'unavailable',
        devices: [],
      })
      .mockResolvedValueOnce({
        currentJoin: {
          status: 'rejected',
          joinId: 'join-1',
          reason: 'invitation_unavailable',
        },
        localMembership: 'unavailable',
        devices: [],
      })

    renderHook(() => useJoinAdmission('join-1', resolved))
    await vi.waitFor(() => expect(getDeviceTrustSnapshot).toHaveBeenCalledTimes(1))

    await act(async () => vi.advanceTimersByTime(1000))

    await vi.waitFor(() =>
      expect(resolved).toHaveBeenCalledWith({
        status: 'rejected',
        joinId: 'join-1',
        reason: 'invitation_unavailable',
      })
    )
  })
})
