import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RedeemResponse, SetupStateResponse } from '@/api/daemon/setupV2'
import {
  acknowledgeSetupCompletion,
  applyIssuedInvitation,
  applyServerSetupState,
  refreshSetupState,
  useSetupRealtimeStore,
} from '@/store/setupRealtimeStore'

const getSetupState = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/setupV2', async () => {
  const actual =
    await vi.importActual<typeof import('@/api/daemon/setupV2')>('@/api/daemon/setupV2')
  return { ...actual, getSetupState }
})

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
    getSetupState.mockReset()
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

    act(() =>
      applyServerSetupState(completedState, { kind: 'pairing_succeeded', role: 'joiner', redeem })
    )

    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'MacBook',
      completion: { kind: 'pairing_succeeded', role: 'joiner', redeem },
    })
  })

  it('moves a completed sponsor into the invitation screen from the issued response', () => {
    const { result } = renderHook(() => useSetupRealtimeStore())
    act(() => applyServerSetupState(completedState, { kind: 'space_ready' }))

    act(() => {
      applyIssuedInvitation({ code: 'ABC123', expiresAtMs: 123_456 })
      applyIssuedInvitation({ code: 'ABC123', expiresAtMs: 123_456 })
    })

    expect(result.current.flow).toEqual({
      kind: 'invitation_pending',
      code: 'ABC123',
      expiresAtMs: 123_456,
      deviceName: 'MacBook',
      completion: { kind: 'space_ready' },
    })
  })

  it('restores the sponsor ready step after an invitation is cancelled or expires', async () => {
    const { result } = renderHook(() => useSetupRealtimeStore())
    act(() => applyServerSetupState(completedState, { kind: 'space_ready' }))
    act(() => applyIssuedInvitation({ code: 'ABC123', expiresAtMs: 123_456 }))
    getSetupState.mockResolvedValue(completedState)

    await act(async () => refreshSetupState())

    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'MacBook',
      completion: { kind: 'space_ready' },
    })
  })

  it('closes only the transient summary when completion is acknowledged', () => {
    const { result } = renderHook(() => useSetupRealtimeStore())
    act(() => applyServerSetupState(completedState, { kind: 'space_ready' }))

    act(() => acknowledgeSetupCompletion())

    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'MacBook',
      completion: null,
    })
  })
})
