import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { useDeviceTrust } from '../device-trust-context'
import { DeviceTrustProvider } from '../DeviceTrustProvider'

const { getDeviceTrust, decideDeviceTrust, subscribe } = vi.hoisted(() => ({
  getDeviceTrust: vi.fn(),
  decideDeviceTrust: vi.fn(),
  subscribe: vi.fn((_topics: string[], _callback: (event: unknown) => void) => vi.fn()),
}))

vi.mock('@/api/daemon/device-trust', () => ({ getDeviceTrust, decideDeviceTrust }))
vi.mock('@/lib/daemon-ws', () => ({ daemonWs: { subscribe } }))
vi.mock('../device-trust-notifications', () => ({ notifyDeviceTrustSnapshot: vi.fn() }))

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
    const pending = { ...emptySnapshot, currentChange: { changeId: 'change-1' } }
    getDeviceTrust.mockResolvedValue(pending)
    decideDeviceTrust.mockRejectedValue(new Error('offline'))
    const { result } = renderHook(() => useDeviceTrust(), { wrapper })
    await waitFor(() => expect(result.current.snapshot).toEqual(pending))
    await act(async () => result.current.decide('apply_change', false))
    expect(decideDeviceTrust).toHaveBeenCalledTimes(1)
    expect(result.current.decisionError).toBeTruthy()
  })
})
