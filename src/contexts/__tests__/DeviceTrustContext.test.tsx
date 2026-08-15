import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'

const { getDeviceTrust, decideDeviceTrust, subscribe } = vi.hoisted(() => ({
  getDeviceTrust: vi.fn(),
  decideDeviceTrust: vi.fn(),
  subscribe: vi.fn((_topics: string[], _callback: (event: unknown) => void) => vi.fn()),
}))

vi.mock('@/api/daemon/device-trust', () => ({ getDeviceTrust, decideDeviceTrust }))
vi.mock('@/lib/daemon-ws', () => ({ daemonWs: { subscribe } }))
vi.mock('@/lib/device-trust-notifications', () => ({ notifyDeviceTrustSnapshot: vi.fn() }))

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

function wrapper({ children }: { children: ReactNode }) {
  return <DeviceTrustProvider enabled>{children}</DeviceTrustProvider>
}

describe('DeviceTrustProvider', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    getDeviceTrust.mockResolvedValue(emptySnapshot)
  })

  it('loads the complete snapshot and reloads it after a trust event', async () => {
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(emptySnapshot))
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    await act(async () => handler({ topic: 'device-trust', eventType: 'device-trust.changed' }))
    await waitFor(() => expect(getDeviceTrust).toHaveBeenCalledTimes(2))
  })

  it('does not automatically repeat a failed user decision', async () => {
    const pending = {
      ...emptySnapshot,
      currentChange: { changeId: 'change-1', allowedChoices: ['apply_change'] },
    }
    getDeviceTrust.mockResolvedValue(pending)
    decideDeviceTrust.mockRejectedValue(new Error('offline'))
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(pending))
    await act(async () => result.current.decide('apply_change', false))
    expect(decideDeviceTrust).toHaveBeenCalledTimes(1)
    expect(result.current.decisionError).toBeTruthy()
  })

  it('exposes the engine request for a second local-removal confirmation', async () => {
    const pending = {
      ...emptySnapshot,
      currentChange: { changeId: 'change-1', allowedChoices: ['apply_change'] },
    }
    getDeviceTrust.mockResolvedValue(pending)
    decideDeviceTrust.mockResolvedValue({
      kind: 'local_device_confirmation_required',
      changeId: 'change-1',
      snapshot: pending,
    })
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(pending))

    await act(async () => result.current.decide('apply_change', false))

    expect(Reflect.get(result.current, 'localRemovalConfirmationChangeId')).toBe('change-1')
  })

  it('does not let an older refresh overwrite a newer snapshot', async () => {
    let resolveFirst!: (snapshot: DeviceTrustSnapshot) => void
    let resolveSecond!: (snapshot: DeviceTrustSnapshot) => void
    getDeviceTrust
      .mockImplementationOnce(
        () => new Promise<DeviceTrustSnapshot>(resolve => (resolveFirst = resolve))
      )
      .mockImplementationOnce(
        () => new Promise<DeviceTrustSnapshot>(resolve => (resolveSecond = resolve))
      )

    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    act(() => handler({ topic: 'device-trust', eventType: 'device-trust.changed' }))

    const newer = { ...emptySnapshot, revision: 2, updatedAtMs: 2 }
    await act(async () => resolveSecond(newer))
    await waitFor(() => expect(result.current.snapshot).toEqual(newer))

    await act(async () => resolveFirst(emptySnapshot))
    expect(result.current.snapshot).toEqual(newer)
  })

  it('keeps a completed decision newer than a refresh started during submission', async () => {
    const pending = {
      ...emptySnapshot,
      currentChange: { changeId: 'change-1', allowedChoices: ['apply_change'] },
    }
    let resolveDecision!: (result: unknown) => void
    let resolveRefresh!: (snapshot: DeviceTrustSnapshot) => void
    getDeviceTrust
      .mockResolvedValueOnce(pending)
      .mockImplementationOnce(
        () => new Promise<DeviceTrustSnapshot>(resolve => (resolveRefresh = resolve))
      )
    decideDeviceTrust.mockImplementationOnce(
      () => new Promise(resolve => (resolveDecision = resolve))
    )

    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(pending))
    let decisionPromise!: Promise<void>
    act(() => {
      decisionPromise = result.current.decide('apply_change', false)
    })
    const handler = subscribe.mock.calls[0]?.[1]
    expect(handler).toBeDefined()
    if (!handler) return
    act(() => handler({ topic: 'device-trust', eventType: 'device-trust.changed' }))

    const decided = { ...emptySnapshot, revision: 3, updatedAtMs: 3 }
    await act(async () =>
      resolveDecision({ kind: 'applied', changeId: 'change-1', snapshot: decided })
    )
    await decisionPromise
    expect(result.current.snapshot).toEqual(decided)

    await act(async () => resolveRefresh({ ...emptySnapshot, revision: 2, updatedAtMs: 2 }))
    expect(result.current.snapshot).toEqual(decided)
    expect(result.current.loading).toBe(false)
  })
})
