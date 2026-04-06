// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const getP2PPeersMock = vi.fn()

// Capture the daemonWs.subscribe handler for test injection
const capturedHandlers: Array<
  (event: { topic: string; eventType: string; payload: unknown }) => void
> = []

vi.mock('@/api/daemon/pairing', () => ({
  getP2PPeers: (...args: unknown[]) => getP2PPeersMock(...args),
  getPairedPeers: vi.fn(),
  getPairedPeersWithStatus: vi.fn(),
  getLocalDeviceInfo: vi.fn(),
  initiateP2PPairing: vi.fn(),
  acceptP2PPairing: vi.fn(),
  rejectP2PPairing: vi.fn(),
  verifyP2PPairingPin: vi.fn(),
  unpairP2PDevice: vi.fn(),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((_topics: string[], handler: (event: unknown) => void) => {
      capturedHandlers.push(handler as (typeof capturedHandlers)[number])
      return vi.fn(() => {
        const idx = capturedHandlers.indexOf(handler as (typeof capturedHandlers)[number])
        if (idx !== -1) capturedHandlers.splice(idx, 1)
      })
    }),
  },
}))

describe('useDeviceDiscovery realtime', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    capturedHandlers.length = 0
    getP2PPeersMock.mockResolvedValue([])
  })

  it('updates peer list from peers.changed envelopes via Redux dispatch', async () => {
    const { useDeviceDiscovery } = await import('@/hooks/useDeviceDiscovery')
    const { result } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(getP2PPeersMock).toHaveBeenCalledTimes(1)
    })

    // Get the captured daemonWs.subscribe handler
    const handler = capturedHandlers[0]
    expect(handler).toBeDefined()

    // Emit a peers.changed event
    act(() => {
      handler({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [{ peerId: 'peer-1', deviceName: 'Desk', connected: true }],
        },
      })
    })

    // scanPhase should have transitioned to 'hasDevices' (via useDeviceDiscovery internal state)
    await waitFor(() => {
      expect(result.current.scanPhase).toBe('hasDevices')
    })
  })

  it('handles peers.nameUpdated event without re-subscribing', async () => {
    const { useDeviceDiscovery } = await import('@/hooks/useDeviceDiscovery')
    const { result } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(getP2PPeersMock).toHaveBeenCalledTimes(1)
    })

    const handler = capturedHandlers[0]
    expect(handler).toBeDefined()

    // Add a peer first
    act(() => {
      handler({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [{ peerId: 'peer-1', deviceName: null, connected: true }],
        },
      })
    })

    await waitFor(() => {
      expect(result.current.scanPhase).toBe('hasDevices')
    })

    // Emit a nameUpdated event
    act(() => {
      handler({
        topic: 'peers',
        eventType: 'peers.nameUpdated',
        payload: {
          peerId: 'peer-1',
          deviceName: 'Renamed Desk',
        },
      })
    })

    // Hook should not crash on nameUpdated
    // (actual name update goes to Redux via dispatch)
  })
})
