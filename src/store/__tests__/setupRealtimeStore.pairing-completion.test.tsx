import { act, renderHook, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
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
})
