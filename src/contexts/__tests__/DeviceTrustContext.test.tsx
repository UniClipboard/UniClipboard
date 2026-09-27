import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeviceGroupChoices, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'

const { getDeviceGroupChoices, chooseDeviceGroup, subscribe } = vi.hoisted(() => ({
  getDeviceGroupChoices: vi.fn(),
  chooseDeviceGroup: vi.fn(),
  subscribe: vi.fn((_topics: string[], _callback: (event: unknown) => void) => vi.fn()),
}))

vi.mock('@/api/daemon/device-trust', () => ({
  getDeviceGroupChoices,
  chooseDeviceGroup,
}))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: { subscribe, onReconnect: () => vi.fn() },
}))

const emptySnapshot: DeviceTrustSnapshot = {
  revision: 1,
  localDeviceId: 'local',
  localMembership: 'active',
  currentChange: null,
  devices: [],
  recovery: 'not_available_in_this_version',
  allowedActions: [],
  blockedReason: null,
  updatedAtMs: 1,
}

const emptyGroups: DeviceGroupChoices = {
  revision: 1,
  deviceTrust: emptySnapshot,
  issues: [],
}

const pendingGroups: DeviceGroupChoices = {
  revision: 7,
  deviceTrust: {
    ...emptySnapshot,
    revision: 7,
    currentChange: {
      changeId: 'change-1',
      proposedByDeviceId: 'peer-a',
      targetDeviceIds: ['peer-b'],
      includesLocalDevice: false,
      applyImpact: {
        usableDeviceIds: ['local'],
        pausedDeviceIds: [],
        localDeviceOutcome: 'active',
        requiresRejoinDeviceIds: ['peer-b'],
      },
      keepCurrentImpact: {
        usableDeviceIds: ['local', 'peer-b'],
        pausedDeviceIds: ['peer-a'],
        localDeviceOutcome: 'active',
        requiresRejoinDeviceIds: [],
      },
      allowedChoices: ['apply_change'],
      blockedReason: null,
    },
  },
  issues: [
    {
      issueId: 'p:issue-1',
      choices: [
        {
          choiceId: 'apply',
          isCurrentGroup: false,
          requiresRePairing: false,
          memberDeviceIds: ['local'],
          membersComplete: true,
        },
      ],
    },
  ],
}

function wrapper({ children }: { children: ReactNode }) {
  return <DeviceTrustProvider enabled>{children}</DeviceTrustProvider>
}

describe('DeviceTrustProvider', () => {
  it('refreshes on focus even when WebKit visibility is stale', async () => {
    const visibility = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden')
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).not.toBeNull())
    const before = getDeviceGroupChoices.mock.calls.length
    await act(async () => window.dispatchEvent(new Event('focus')))
    expect(getDeviceGroupChoices.mock.calls.length).toBe(before + 1)
    visibility.mockRestore()
  })
  beforeEach(() => {
    vi.clearAllMocks()
    getDeviceGroupChoices.mockResolvedValue(emptyGroups)
  })

  it('waits for a new refresh request after the initial check fails', async () => {
    getDeviceGroupChoices.mockRejectedValueOnce(new Error('temporarily unavailable'))
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })

    await waitFor(() => expect(result.current.refreshError).toBe('temporarily unavailable'))

    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(1)
    expect(result.current.deviceGroups).toBeNull()

    await act(async () => window.dispatchEvent(new Event('focus')))
    await waitFor(() => expect(result.current.deviceGroups).toEqual(emptyGroups))
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2)
  })

  it('ignores websocket events that do not invalidate device groups', async () => {
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(emptySnapshot))
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    await act(async () => handler({ topic: 'system', eventType: 'system.snapshot' }))
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(1)
  })

  it('skips a device change revision that is already loaded', async () => {
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(emptySnapshot))
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    await act(async () =>
      handler({
        topic: 'device-trust',
        eventType: 'device-trust.changed',
        payload: { revision: 1 },
      })
    )
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(1)
  })

  it('loads complete choices and refreshes after device or global invalidation events', async () => {
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(emptySnapshot))
    expect(result.current.deviceGroups).toEqual(emptyGroups)
    expect(subscribe).toHaveBeenCalledWith(['device-trust', 'system'], expect.any(Function))

    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    await act(async () => handler({ topic: 'system', eventType: 'system.refresh_required' }))
    await waitFor(() => expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2))
  })

  it('keeps the unlock and recovery meanings of a failed device group query', async () => {
    getDeviceGroupChoices
      .mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /member/device-group-choices', {
          code: 'device_group_choices_unlock_required',
          message: 'unlock this space to read device groups',
        })
      )
      .mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /member/device-group-choices', {
          code: 'device_group_choices_recovery_required',
          message: 'space membership needs recovery',
        })
      )
      .mockResolvedValueOnce(emptyGroups)
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.refreshFailure).toBe('unlock_required'))

    await act(async () => result.current.refresh())
    expect(result.current.refreshFailure).toBe('recovery_required')

    await act(async () => result.current.refresh())
    expect(result.current.refreshFailure).toBeNull()
    expect(result.current.deviceGroups).toEqual(emptyGroups)
  })

  it('treats device-trust events only as invalidation signals', async () => {
    const refreshedGroups: DeviceGroupChoices = {
      revision: 2,
      deviceTrust: {
        ...emptySnapshot,
        revision: 2,
        spaceDeviceUpdate: { phase: 'completed' },
        devices: [
          {
            deviceId: 'removed-peer',
            displayName: 'Removed Peer',
            isLocal: false,
            reachability: 'offline',
            membership: 'removed',
            groupRelationship: 'awaiting_removal_acknowledgement',
            compatibility: 'compatible',
            syncRelationship: 'removed_peer_device',
            availableActions: [],
            blockedReason: null,
          },
        ],
      },
      issues: [],
    }
    getDeviceGroupChoices.mockResolvedValueOnce(emptyGroups).mockResolvedValueOnce(refreshedGroups)
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(emptyGroups))
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return

    await act(async () =>
      handler({
        topic: 'device-trust',
        eventType: 'device-trust.changed',
        payload: { revision: 2, devices: [] },
      })
    )

    await waitFor(() => expect(result.current.deviceGroups).toEqual(refreshedGroups))
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2)
    expect(result.current.snapshot?.devices[0]?.groupRelationship).toBe(
      'awaiting_removal_acknowledgement'
    )
  })

  it('submits opaque ids with the query revision and then refreshes', async () => {
    getDeviceGroupChoices.mockResolvedValueOnce(pendingGroups).mockResolvedValueOnce(emptyGroups)
    chooseDeviceGroup.mockResolvedValue({
      outcome: 'completed',
      currentRevision: null,
    })
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(pendingGroups))

    await act(async () => result.current.choose('p:issue-1', 'apply', false))

    expect(chooseDeviceGroup).toHaveBeenCalledWith('p:issue-1', 'apply', 7, false)
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2)
    expect(result.current.deviceGroups).toEqual(emptyGroups)
  })

  it('exposes a second local-removal confirmation for the same issue', async () => {
    getDeviceGroupChoices.mockResolvedValue(pendingGroups)
    chooseDeviceGroup.mockResolvedValue({
      outcome: 'local_device_confirmation_required',
      currentRevision: null,
    })
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(pendingGroups))

    await act(async () => result.current.choose('p:issue-1', 'apply', false))

    expect(result.current.localRemovalConfirmationIssueId).toBe('p:issue-1')
    expect(result.current.localRemovalConfirmationChoiceId).toBe('apply')
  })

  it('does not automatically repeat a failed user choice', async () => {
    getDeviceGroupChoices.mockResolvedValue(pendingGroups)
    chooseDeviceGroup.mockRejectedValue(new Error('offline'))
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(pendingGroups))

    await act(async () => result.current.choose('p:issue-1', 'apply', false))

    expect(chooseDeviceGroup).toHaveBeenCalledTimes(1)
    expect(result.current.decisionError).toBeTruthy()
  })

  it('serializes refreshes and coalesces repeated invalidations', async () => {
    let resolveFirst!: (state: DeviceGroupChoices) => void
    let resolveSecond!: (state: DeviceGroupChoices) => void
    getDeviceGroupChoices
      .mockImplementationOnce(
        () => new Promise<DeviceGroupChoices>(resolve => (resolveFirst = resolve))
      )
      .mockImplementationOnce(
        () => new Promise<DeviceGroupChoices>(resolve => (resolveSecond = resolve))
      )

    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    act(() => {
      handler({ topic: 'system', eventType: 'system.refresh_required' })
      handler({ topic: 'system', eventType: 'system.refresh_required' })
    })
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(1)

    await act(async () => resolveFirst(emptyGroups))
    await waitFor(() => expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2))

    const newer = {
      ...emptyGroups,
      revision: 2,
      deviceTrust: { ...emptySnapshot, revision: 2, updatedAtMs: 2 },
    }
    await act(async () => resolveSecond(newer))
    await waitFor(() => expect(result.current.deviceGroups).toEqual(newer))
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2)
  })

  it('runs a queued refresh after the active refresh fails', async () => {
    let rejectFirst!: (error: Error) => void
    getDeviceGroupChoices
      .mockImplementationOnce(
        () => new Promise<DeviceGroupChoices>((_resolve, reject) => (rejectFirst = reject))
      )
      .mockResolvedValueOnce(emptyGroups)

    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return

    act(() => handler({ topic: 'system', eventType: 'system.refresh_required' }))
    expect(getDeviceGroupChoices).toHaveBeenCalledTimes(1)

    await act(async () => rejectFirst(new Error('offline')))
    await waitFor(() => expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(result.current.deviceGroups).toEqual(emptyGroups))
  })
})
