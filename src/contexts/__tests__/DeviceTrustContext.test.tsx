import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeviceGroupChoices, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'

const { getDeviceGroupChoices, chooseDeviceGroup, subscribe } = vi.hoisted(() => ({
  getDeviceGroupChoices: vi.fn(),
  chooseDeviceGroup: vi.fn(),
  subscribe: vi.fn((_topics: string[], _callback: (event: unknown) => void) => vi.fn()),
}))

vi.mock('@/api/daemon/device-trust', () => ({ getDeviceGroupChoices, chooseDeviceGroup }))
vi.mock('@/lib/daemon-ws', () => ({ daemonWs: { subscribe } }))

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
  beforeEach(() => {
    vi.clearAllMocks()
    getDeviceGroupChoices.mockResolvedValue(emptyGroups)
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

  it('does not let an older refresh overwrite a newer response', async () => {
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
    act(() => handler({ topic: 'device-trust', eventType: 'device-trust.changed' }))

    const newer = {
      ...emptyGroups,
      revision: 2,
      deviceTrust: { ...emptySnapshot, revision: 2, updatedAtMs: 2 },
    }
    await act(async () => resolveSecond(newer))
    await waitFor(() => expect(result.current.deviceGroups).toEqual(newer))

    await act(async () => resolveFirst(emptyGroups))
    expect(result.current.deviceGroups).toEqual(newer)
  })
})
