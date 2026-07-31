import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SetupPairingCompletedEvent } from '@/api/setupEvents'

const mocks = vi.hoisted(() => ({
  getSetupState: vi.fn(),
  pairingCompleted: null as ((event: SetupPairingCompletedEvent) => void) | null,
}))

vi.mock('@/api/daemon/setupV2', async () => {
  const actual =
    await vi.importActual<typeof import('@/api/daemon/setupV2')>('@/api/daemon/setupV2')
  return { ...actual, getSetupState: mocks.getSetupState }
})

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/api/setupEvents', () => ({
  onSetupInvitationIssued: vi.fn().mockResolvedValue(() => undefined),
  onSetupInvitationRevoked: vi.fn().mockResolvedValue(() => undefined),
  onSetupPairingCompleted: vi
    .fn()
    .mockImplementation(async (callback: (event: SetupPairingCompletedEvent) => void) => {
      mocks.pairingCompleted = callback
      return () => undefined
    }),
}))

describe('setupRealtimeStore pairing completion', () => {
  beforeEach(() => {
    vi.resetModules()
    mocks.getSetupState.mockReset()
    mocks.pairingCompleted = null
  })

  it('leaves the invitation screen immediately when pairing succeeds', async () => {
    const invitationState = {
      hasCompleted: true,
      currentInvitation: { code: 'ABC123', expiresAtMs: 123_456 },
      deviceName: 'Host Mac',
    }
    mocks.getSetupState.mockResolvedValue(invitationState)

    const { useSetupRealtimeStore } = await import('@/store/setupRealtimeStore')
    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => expect(mocks.pairingCompleted).toBeTypeOf('function'))
    expect(result.current.flow.kind).toBe('invitation_pending')

    act(() => {
      mocks.pairingCompleted?.({
        sponsorDeviceId: 'host-id',
        joinerDeviceId: 'new-device-id',
        success: true,
        reason: null,
      })
    })

    await waitFor(() => expect(result.current.flow.kind).toBe('completed'))
    expect(result.current.flow).toMatchObject({
      completion: {
        kind: 'pairing_succeeded',
        role: 'sponsor',
        sponsorDeviceId: 'host-id',
        peerDeviceId: 'new-device-id',
      },
    })
  })

  it('leaves a consumed invitation screen when pairing fails', async () => {
    const completedState = {
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'Host Mac',
    }
    mocks.getSetupState.mockResolvedValue(completedState)

    const { applyIssuedInvitation, applyServerSetupState, useSetupRealtimeStore } =
      await import('@/store/setupRealtimeStore')
    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => expect(mocks.pairingCompleted).toBeTypeOf('function'))
    act(() => {
      applyServerSetupState(completedState, { kind: 'space_ready' })
      applyIssuedInvitation({ code: 'ABC123', expiresAtMs: 123_456 })
    })
    expect(result.current.flow.kind).toBe('invitation_pending')

    act(() => {
      mocks.pairingCompleted?.({
        sponsorDeviceId: 'host-id',
        joinerDeviceId: null,
        success: false,
        reason: 'passphrase_mismatch',
      })
    })

    await waitFor(() => expect(mocks.getSetupState).toHaveBeenCalledTimes(2))
    expect(result.current.flow).toEqual({
      kind: 'completed',
      deviceName: 'Host Mac',
      completion: { kind: 'space_ready' },
    })
  })
})
