import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'

const { getDeviceGroupChoices, chooseDeviceGroup, subscribe } = vi.hoisted(() => ({
  getDeviceGroupChoices: vi.fn(),
  chooseDeviceGroup: vi.fn(),
  subscribe: vi.fn((_topics: string[], _callback: (event: unknown) => void) => vi.fn()),
}))

vi.mock('@/api/daemon/device-trust', () => ({ getDeviceGroupChoices, chooseDeviceGroup }))
vi.mock('@/lib/daemon-ws', () => ({ daemonWs: { subscribe, onReconnect: () => vi.fn() } }))

const pending: DeviceGroupChoices = {
  revision: 38,
  deviceTrust: {
    revision: 38,
    localDeviceId: 'a',
    localMembership: 'active',
    currentChange: null,
    devices: [],
    recovery: 'not_available_in_this_version',
    allowedActions: [],
    blockedReason: null,
    updatedAtMs: 0,
  },
  issues: [
    {
      issueId: 'c:handoff',
      choices: [
        {
          choiceId: 'b:local',
          isCurrentGroup: true,
          requiresRePairing: false,
          memberDeviceIds: ['a'],
          membersComplete: true,
        },
      ],
    },
  ],
}
const completed: DeviceGroupChoices = { ...pending, revision: 39, issues: [] }

function wrapper({ children }: { children: ReactNode }) {
  return <DeviceTrustProvider enabled>{children}</DeviceTrustProvider>
}

describe('handoff: choice completion overlapping refresh', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    subscribe.mockReturnValue(vi.fn())
    chooseDeviceGroup.mockResolvedValue({ outcome: 'completed', currentRevision: null })
  })

  it('clears busy when no notification overtakes the completion read', async () => {
    getDeviceGroupChoices.mockResolvedValueOnce(pending).mockResolvedValue(completed)
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(pending))
    await act(async () => result.current.choose('c:handoff', 'b:local', false))
    expect(result.current.deviceGroups?.issues).toHaveLength(0)
    expect(result.current.decisionBusy).toBe(false)
    expect(result.current.decision?.outcome).toBe('completed')
    expect(result.current.decision?.groups.issues[0].issueId).toBe('c:handoff')
  })

  it('never upgrades an older rendered choice to a newer revision silently', async () => {
    getDeviceGroupChoices.mockResolvedValue(pending)
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(pending))
    const oldChoose = result.current.choose
    getDeviceGroupChoices.mockResolvedValue({ ...pending, revision: 40 })
    await act(async () => result.current.refresh())
    await act(async () => oldChoose('c:handoff', 'b:local', false))
    expect(chooseDeviceGroup).not.toHaveBeenCalled()
    expect(result.current.decisionError).toBe('device_state_changed')
    getDeviceGroupChoices.mockResolvedValue(completed)
    await act(async () => result.current.refresh())
    expect(result.current.decisionError).toBeNull()
  })

  it('does not dismiss the result when its final verification fails', async () => {
    getDeviceGroupChoices.mockResolvedValueOnce(pending).mockResolvedValueOnce(completed)
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.deviceGroups).toEqual(pending))
    await act(async () => result.current.choose('c:handoff', 'b:local', false))
    getDeviceGroupChoices.mockRejectedValueOnce(new Error('offline'))
    await act(async () => result.current.acknowledgeDecision?.())
    expect(result.current.decision).not.toBeNull()
    expect(result.current.decisionError).toBeTruthy()
  })

  it.each(['before', 'after'] as const)(
    'clears busy when notification response finishes %s the choice read',
    async order => {
      let finishChoiceRead!: (value: DeviceGroupChoices) => void
      let finishNotificationRead!: (value: DeviceGroupChoices) => void
      getDeviceGroupChoices
        .mockResolvedValueOnce(pending)
        .mockImplementationOnce(
          () =>
            new Promise<DeviceGroupChoices>(resolve => {
              finishChoiceRead = resolve
            })
        )
        .mockImplementationOnce(
          () =>
            new Promise<DeviceGroupChoices>(resolve => {
              finishNotificationRead = resolve
            })
        )
        .mockResolvedValue(completed)
      const { result } = renderHook(() => useDeviceTrust(), { wrapper })
      await waitFor(() => expect(result.current.deviceGroups).toEqual(pending))
      let submitted!: Promise<void>
      await act(async () => {
        submitted = result.current.choose('c:handoff', 'b:local', false)
      })
      expect(result.current.decisionBusy).toBe(true)
      expect(getDeviceGroupChoices).toHaveBeenCalledTimes(2)
      const notify = subscribe.mock.calls[0][1]
      await act(async () => notify({ topic: 'device-trust', eventType: 'device-trust.changed' }))
      expect(getDeviceGroupChoices).toHaveBeenCalledTimes(3)
      if (order === 'before') {
        await act(async () => finishNotificationRead(completed))
        await act(async () => {
          finishChoiceRead(completed)
          await submitted
        })
      } else {
        await act(async () => {
          finishChoiceRead(completed)
          await submitted
        })
        await act(async () => finishNotificationRead(completed))
      }
      expect(chooseDeviceGroup).toHaveBeenCalledTimes(1)
      expect(result.current.deviceGroups?.issues).toHaveLength(0)
      expect(result.current.loading).toBe(false)
      expect(result.current.decisionBusy).toBe(false)
    }
  )
})
