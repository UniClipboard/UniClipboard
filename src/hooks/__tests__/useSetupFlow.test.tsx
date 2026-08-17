import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useSetupFlow } from '@/hooks/useSetupFlow'
import type { SetupFlow } from '@/store/setupRealtimeStore'

const getDeviceTrust = vi.hoisted(() => vi.fn())
const getSetupState = vi.hoisted(() => vi.fn())
const issuePairingInvitation = vi.hoisted(() => vi.fn())
const redeemInvitation = vi.hoisted(() => vi.fn())
const applyIssuedInvitation = vi.hoisted(() => vi.fn())
const applyServerSetupState = vi.hoisted(() => vi.fn())
const subscribe = vi.hoisted(() => vi.fn())
const onReconnect = vi.hoisted(() => vi.fn())

let deviceTrustHandler: ((event: { eventType: string }) => void) | undefined
let reconnectHandler: (() => void) | undefined
let flow: SetupFlow = {
  kind: 'completed' as const,
  deviceName: 'MacBook',
  completion: { kind: 'space_ready' as const },
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('sonner', () => ({ toast: { error: vi.fn() } }))

vi.mock('@/api/daemon/device-trust', () => ({
  getDeviceTrust: () => getDeviceTrust(),
}))

vi.mock('@/api/daemon/setupV2', () => ({
  cancelInvitation: vi.fn(),
  getSetupState: () => getSetupState(),
  initializeSpace: vi.fn(),
  issuePairingInvitation: () => issuePairingInvitation(),
  redeemInvitation: (...args: unknown[]) => redeemInvitation(...args),
  resetSetup: vi.fn(),
  SetupV2Error: class SetupV2Error extends Error {},
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: (...args: unknown[]) => subscribe(...args),
    onReconnect: (...args: unknown[]) => onReconnect(...args),
  },
}))

vi.mock('@/lib/wdio-test-bridge', () => ({ recordWdioE2eEvent: vi.fn() }))

vi.mock('@/store/setupRealtimeStore', () => ({
  acknowledgeSetupCompletion: vi.fn(),
  applyIssuedInvitation: (...args: unknown[]) => applyIssuedInvitation(...args),
  applyServerSetupState: (...args: unknown[]) => applyServerSetupState(...args),
  refreshSetupState: vi.fn(),
  useSetupRealtimeStore: () => ({ flow, hydrated: true }),
}))

describe('useSetupFlow sponsor pairing completion', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    deviceTrustHandler = undefined
    reconnectHandler = undefined
    flow = {
      kind: 'completed',
      deviceName: 'MacBook',
      completion: { kind: 'space_ready' },
    }
    subscribe.mockImplementation((_topics, callback) => {
      deviceTrustHandler = callback
      return () => undefined
    })
    onReconnect.mockImplementation(callback => {
      reconnectHandler = callback
      return () => undefined
    })
    getDeviceTrust.mockResolvedValue({
      localDeviceId: 'local',
      devices: [{ deviceId: 'local', membership: 'active' }],
    })
    getSetupState.mockResolvedValue({
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'MacBook',
    })
    issuePairingInvitation.mockResolvedValue({
      code: '123456789',
      expiresAtMs: 123_456,
    })
  })

  it('moves the sponsor to pairing complete after the issued invitation admits a device', async () => {
    const { result, rerender } = renderHook(() => useSetupFlow())

    await act(async () => {
      await result.current.issueInvitation()
    })

    flow = {
      kind: 'invitation_pending',
      code: '123456789',
      expiresAtMs: 123_456,
      deviceName: 'MacBook',
      completion: { kind: 'space_ready' },
    }
    rerender()
    getDeviceTrust.mockResolvedValue({
      localDeviceId: 'local',
      devices: [
        { deviceId: 'local', membership: 'active' },
        { deviceId: 'peer', membership: 'active' },
      ],
    })

    await waitFor(() => expect(deviceTrustHandler).toBeTypeOf('function'))
    act(() => deviceTrustHandler?.({ eventType: 'device-trust.changed' }))

    await waitFor(() => {
      expect(applyServerSetupState).toHaveBeenCalledWith(
        expect.objectContaining({ currentInvitation: null }),
        {
          kind: 'pairing_succeeded',
          role: 'sponsor',
          sponsorDeviceId: 'local',
          peerDeviceId: 'peer',
        }
      )
    })
  })

  it('rechecks sponsor completion after a WebSocket reconnect', async () => {
    const { result, rerender } = renderHook(() => useSetupFlow())
    await act(async () => result.current.issueInvitation())

    flow = {
      kind: 'invitation_pending',
      code: '123456789',
      expiresAtMs: 123_456,
      deviceName: 'MacBook',
      completion: { kind: 'space_ready' },
    }
    rerender()
    getDeviceTrust.mockResolvedValue({
      localDeviceId: 'local',
      devices: [
        { deviceId: 'local', membership: 'active' },
        { deviceId: 'peer', membership: 'active' },
      ],
    })

    await waitFor(() => expect(reconnectHandler).toBeTypeOf('function'))
    act(() => reconnectHandler?.())

    await waitFor(() => expect(applyServerSetupState).toHaveBeenCalledTimes(1))
  })
})

describe('useSetupFlow joiner admission', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    flow = { kind: 'entry' }
    redeemInvitation.mockResolvedValue({
      status: 'pending',
      joinId: 'join-123',
      targetSpaceId: 'space-123',
      sponsorDeviceId: 'sponsor-123',
      sponsorIdentityFingerprint: 'fingerprint',
      cancelRequested: false,
    })
  })

  it('keeps a pending admission visible with its join id', async () => {
    const { result } = renderHook(() => useSetupFlow())

    act(() => result.current.startJoinSpace())
    await act(async () => {
      await result.current.redeemInvitation({
        code: 'ABCD1234',
        passphrase: 'passphrase',
      })
    })

    expect(result.current.screen).toEqual({
      kind: 'join_pending',
      joinId: 'join-123',
    })
  })

  it('uses the matching durable admission result after a device-trust update', async () => {
    const { result } = renderHook(() => useSetupFlow())
    act(() => result.current.startJoinSpace())
    await act(async () => {
      await result.current.redeemInvitation({
        code: 'ABCD1234',
        passphrase: 'passphrase',
      })
    })
    getDeviceTrust.mockResolvedValue({
      currentJoin: {
        status: 'active',
        joinId: 'join-123',
        joinedSpace: {
          sponsorDeviceId: 'sponsor-123',
          sponsorIdentityFingerprint: 'fingerprint',
          spaceId: 'space-123',
          selfDeviceId: 'local-123',
          selfIdentityFingerprint: 'local-fingerprint',
          migratedRecords: null,
          preservedUnreadableRecords: null,
        },
      },
    })

    await waitFor(() => expect(deviceTrustHandler).toBeTypeOf('function'))
    act(() => deviceTrustHandler?.({ eventType: 'device-trust.changed' }))

    await waitFor(() => {
      expect(applyServerSetupState).toHaveBeenCalledWith(
        expect.objectContaining({ hasCompleted: true }),
        expect.objectContaining({ kind: 'pairing_succeeded', role: 'joiner' })
      )
    })
  })
})
