import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RedeemResponse, SetupStateResponse } from '@/api/daemon/setupV2'
import {
  acknowledgeSetupCompletion,
  applyServerSetupState,
  useSetupRealtimeStore,
} from '@/store/setupRealtimeStore'

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn(() => new Promise<void>(() => {})),
}))

const entryState: SetupStateResponse = {
  hasCompleted: false,
  currentInvitation: null,
  deviceName: null,
}

const completedState: SetupStateResponse = {
  hasCompleted: true,
  currentInvitation: null,
  deviceName: 'MacBook',
}

const redeem: RedeemResponse = {
  sponsorDeviceId: 'sponsor-id',
  sponsorIdentityFingerprint: 'sponsor-fingerprint',
  spaceId: 'space-id',
  selfDeviceId: 'self-id',
  selfIdentityFingerprint: 'self-fingerprint',
}

describe('setupRealtimeStore completion ownership', () => {
  beforeEach(() => {
    act(() => applyServerSetupState(entryState))
  })

  it('does not create a pending completion summary for an already-completed device', () => {
    const { result } = renderHook(() => useSetupRealtimeStore())

    act(() => applyServerSetupState(completedState))

    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'MacBook',
      completion: null,
    })
  })

  it('keeps joiner completion data in the same snapshot as the completed flow', () => {
    const { result } = renderHook(() => useSetupRealtimeStore())

    act(() => applyServerSetupState(completedState, { role: 'joiner', redeem }))

    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'MacBook',
      completion: { role: 'joiner', redeem },
    })
  })

  it('closes only the transient summary when completion is acknowledged', () => {
    const { result } = renderHook(() => useSetupRealtimeStore())
    act(() => applyServerSetupState(completedState, { role: 'sponsor' }))

    act(() => acknowledgeSetupCompletion())

    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'MacBook',
      completion: null,
    })
  })
})
