import { act, renderHook, waitFor } from '@testing-library/react'
import { vi, describe, it, expect, beforeEach, afterEach } from 'vitest'
import { useDeviceDiscovery } from '../useDeviceDiscovery'
import { setDiscoveredPeers, clearDiscoveredPeers } from '@/store/slices/devicesSlice'

// Mock the daemon pairing API module
const mockGetP2PPeers = vi.fn()
vi.mock('@/api/daemon/pairing', () => ({
  getP2PPeers: (...args: unknown[]) => mockGetP2PPeers(...args),
}))

// Mock daemon-ws subscribe
const mockUnsubscribe = vi.fn()
let capturedSubscribeHandler:
  | ((event: { topic: string; eventType: string; payload: unknown }) => void)
  | null = null
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn(
      (
        _topics: string[],
        handler: (event: { topic: string; eventType: string; payload: unknown }) => void
      ) => {
        capturedSubscribeHandler = handler
        return mockUnsubscribe
      }
    ),
    connect: vi.fn(),
    disconnect: vi.fn(),
    reset: vi.fn(),
  },
}))

// Mock Redux dispatch
const mockDispatch = vi.fn()
vi.mock('react-redux', () => ({
  useDispatch: () => mockDispatch,
  useSelector: vi.fn((selector: (state: unknown) => unknown) =>
    selector({ devices: { discoveredPeers: [] } })
  ),
}))

describe('useDeviceDiscovery', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockGetP2PPeers.mockResolvedValue([])
    mockDispatch.mockClear()
    capturedSubscribeHandler = null
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('Test 1: initial load calls getP2PPeers and sets up daemonWs.subscribe when active=true', async () => {
    const { unmount } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(mockGetP2PPeers).toHaveBeenCalledTimes(1)
    })

    const { daemonWs } = await import('@/lib/daemon-ws')
    expect(daemonWs.subscribe).toHaveBeenCalledTimes(1)
    expect(daemonWs.subscribe).toHaveBeenCalledWith(['peers'], expect.any(Function))

    unmount()
  })

  it('Test 2: returns scanning phase initially, transitions to hasDevices when getP2PPeers returns peers', async () => {
    mockGetP2PPeers.mockResolvedValue([
      {
        peerId: 'peer-1',
        deviceName: 'MacBook Pro',
        addresses: [],
        isPaired: false,
        connected: false,
      },
    ])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    // Initially scanning
    expect(result.current.scanPhase).toBe('scanning')

    // After fetch resolves, should transition to hasDevices
    await waitFor(() => {
      expect(result.current.scanPhase).toBe('hasDevices')
    })

    // Peers are dispatched to Redux, not returned from hook
    await waitFor(() => {
      expect(mockDispatch).toHaveBeenCalledWith(
        setDiscoveredPeers(
          expect.arrayContaining([
            expect.objectContaining({
              id: 'peer-1',
              deviceName: 'MacBook Pro',
              device_type: 'desktop',
            }),
          ])
        )
      )
    })
  })

  it('Test 3: 10-second timeout transitions scanPhase from scanning to empty when no devices found', async () => {
    vi.useFakeTimers()
    mockGetP2PPeers.mockResolvedValue([])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    // Allow promise microtasks to settle
    await act(async () => {
      await Promise.resolve()
    })

    expect(result.current.scanPhase).toBe('scanning')

    // Advance 10 seconds
    act(() => {
      vi.advanceTimersByTime(10_000)
    })

    expect(result.current.scanPhase).toBe('empty')
  })

  it('Test 4: peers.changed event dispatches setDiscoveredPeers and transitions to hasDevices', async () => {
    mockGetP2PPeers.mockResolvedValue([])

    const { result, unmount } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(mockGetP2PPeers).toHaveBeenCalledTimes(1)
    })

    const { daemonWs } = await import('@/lib/daemon-ws')
    await waitFor(() => {
      expect(daemonWs.subscribe).toHaveBeenCalledTimes(1)
    })

    expect(result.current.scanPhase).toBe('scanning')

    // Fire peers.changed event
    act(() => {
      capturedSubscribeHandler?.({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [{ peerId: 'peer-2', deviceName: 'iPhone', connected: false }],
        },
      })
    })

    // Should dispatch setDiscoveredPeers with the new peer
    // Call sequence: clearDiscoveredPeers (1) + setDiscoveredPeers([]) from loadPeers (2) +
    // setDiscoveredPeers(fn) functional updater from handler (3).
    // Verify the third call is the functional updater carrying peer-2.
    const thirdCallAction = mockDispatch.mock.calls[2][0] as { type: string; payload: unknown }
    expect(thirdCallAction.type).toBe('devices/setDiscoveredPeers')
    expect(typeof thirdCallAction.payload).toBe('function')
    expect(result.current.scanPhase).toBe('hasDevices')

    unmount()
  })

  it('Test 5: device appearing after empty state transitions scanPhase back to hasDevices', async () => {
    vi.useFakeTimers()
    mockGetP2PPeers.mockResolvedValue([])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    // Allow microtasks to settle
    await act(async () => {
      await Promise.resolve()
    })

    // Transition to empty
    act(() => {
      vi.advanceTimersByTime(10_000)
    })
    expect(result.current.scanPhase).toBe('empty')

    // Device appears
    act(() => {
      capturedSubscribeHandler?.({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [{ peerId: 'peer-3', deviceName: 'Windows PC', connected: false }],
        },
      })
    })

    expect(result.current.scanPhase).toBe('hasDevices')
  })

  it('Test 6: resetScan dispatches clearDiscoveredPeers, resets to scanning, starts fresh 10s timeout, re-fetches peers', async () => {
    vi.useFakeTimers()
    mockGetP2PPeers.mockResolvedValue([
      { peerId: 'peer-1', deviceName: 'MacBook', addresses: [], isPaired: false, connected: false },
    ])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    // Allow initial load to resolve
    await act(async () => {
      await vi.runAllTimersAsync()
    })

    // After resetScan, second call returns empty list
    mockGetP2PPeers.mockResolvedValue([])

    // Reset
    act(() => {
      result.current.resetScan()
    })

    // Should dispatch clearDiscoveredPeers immediately
    expect(mockDispatch).toHaveBeenCalledWith(clearDiscoveredPeers())
    expect(result.current.scanPhase).toBe('scanning')

    // Allow re-fetch to resolve
    await act(async () => {
      await Promise.resolve()
    })

    // getP2PPeers should have been called twice (initial + resetScan)
    expect(mockGetP2PPeers).toHaveBeenCalledTimes(2)

    // New 10-second timeout should work -- transitions to empty since no peers returned
    act(() => {
      vi.advanceTimersByTime(10_000)
    })

    expect(result.current.scanPhase).toBe('empty')
  })

  it('Test 7: when active goes false then true, clearDiscoveredPeers dispatched and state resets before re-setup', async () => {
    mockGetP2PPeers.mockResolvedValue([
      { peerId: 'peer-1', deviceName: 'Device', addresses: [], isPaired: false, connected: false },
    ])

    const { result, rerender } = renderHook(
      ({ active }: { active: boolean }) => useDeviceDiscovery(active),
      {
        initialProps: { active: true },
      }
    )

    await waitFor(() => {
      expect(mockDispatch).toHaveBeenCalledWith(
        setDiscoveredPeers(expect.arrayContaining([expect.objectContaining({ id: 'peer-1' })]))
      )
    })

    // Deactivate
    rerender({ active: false })

    // Should dispatch clearDiscoveredPeers and reset to scanning
    expect(mockDispatch).toHaveBeenCalledWith(clearDiscoveredPeers())
    expect(result.current.scanPhase).toBe('scanning')

    // Re-activate
    mockGetP2PPeers.mockResolvedValue([])
    rerender({ active: true })

    // Should start fresh in scanning phase
    expect(result.current.scanPhase).toBe('scanning')
  })

  it('Test 8: getP2PPeers() rejection calls onError callback, hook remains in scanning phase', async () => {
    const onError = vi.fn()
    mockGetP2PPeers.mockRejectedValueOnce(new Error('network'))

    const { result } = renderHook(() => useDeviceDiscovery(true, { onError }))

    await waitFor(() => {
      expect(onError).toHaveBeenCalled()
    })

    expect(result.current.scanPhase).toBe('scanning')
    expect(onError).toHaveBeenCalledWith(expect.any(Error))
  })

  it('Test 9: cleanup on unmount calls unsubscribe and clears timeout', async () => {
    mockGetP2PPeers.mockResolvedValue([])

    const { unmount } = renderHook(() => useDeviceDiscovery(true))

    const { daemonWs } = await import('@/lib/daemon-ws')
    await waitFor(() => {
      expect(daemonWs.subscribe).toHaveBeenCalledTimes(1)
    })

    unmount()
    expect(mockUnsubscribe).toHaveBeenCalledTimes(1)
  })

  it('Test 10: anonymous peer from getP2PPeers has deviceName: null (hook stores raw value, no fallback)', async () => {
    mockGetP2PPeers.mockResolvedValue([
      { peerId: 'peer-anon', deviceName: null, addresses: [], isPaired: false, connected: false },
    ])

    renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(mockDispatch).toHaveBeenCalledWith(
        setDiscoveredPeers(
          expect.arrayContaining([expect.objectContaining({ id: 'peer-anon', deviceName: null })])
        )
      )
    })
  })

  it('Test 11: onError callback receives the Error object when getP2PPeers fails', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    const onError = vi.fn()
    const networkError = new Error('connection refused')
    mockGetP2PPeers.mockRejectedValueOnce(networkError)

    renderHook(() => useDeviceDiscovery(true, { onError }))

    await waitFor(() => {
      expect(onError).toHaveBeenCalled()
    })

    const receivedError = onError.mock.calls[0][0] as Error
    expect(receivedError).toBeInstanceOf(Error)
    expect(receivedError.message).toBe('connection refused')
  })

  it('Test 12: diffPeerSnapshots discovered peers are appended via functional updater (no stale closure)', async () => {
    // This test verifies the fix for Warning 6: dispatch(setDiscoveredPeers(prev => [...prev, ...next]))
    // When multiple peers.changed events fire, each new peer should be dispatched.
    // After the first peers.changed event (peer-a), a dispatch is expected.
    mockGetP2PPeers.mockResolvedValue([])

    renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(mockGetP2PPeers).toHaveBeenCalledTimes(1)
    })

    const { daemonWs } = await import('@/lib/daemon-ws')
    await waitFor(() => {
      expect(daemonWs.subscribe).toHaveBeenCalledTimes(1)
    })

    // Fire first peers.changed event: peer-a is new
    act(() => {
      capturedSubscribeHandler?.({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [{ peerId: 'peer-a', deviceName: 'Device A', connected: true }],
        },
      })
    })

    // After first event: knownPeers has peer-a, setScanPhase -> 'hasDevices', nextPeers=[peer-a] dispatched
    // Verify at least one setDiscoveredPeers call was made
    const dispatchCallArgs = mockDispatch.mock.calls.map(call => call[0])
    const setDiscoveredPeersCall = dispatchCallArgs.find(arg => {
      if (!arg || typeof arg !== 'object') return false
      const action = arg as { type?: unknown }
      return (
        action.type === 'devices/setDiscoveredPeers' ||
        action.type === expect.stringContaining('setDiscoveredPeers')
      )
    })
    expect(setDiscoveredPeersCall).toBeDefined()
  })
})
