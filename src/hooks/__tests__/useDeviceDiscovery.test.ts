import { act, renderHook, waitFor } from '@testing-library/react'
import { vi, describe, it, expect, beforeEach, afterEach } from 'vitest'
import { useDeviceDiscovery } from '../useDeviceDiscovery'
import * as p2pApi from '@/api/p2p'

// Mock the p2p API module
const mockGetP2PPeers = vi.fn()
const mockDiscoveryUnlisten = vi.fn()
const mockConnectionUnlisten = vi.fn()
const mockNameUnlisten = vi.fn()
let capturedDiscoveryCb: (event: unknown) => void

vi.mock('@/api/p2p', () => ({
  getP2PPeers: (...args: unknown[]) => mockGetP2PPeers(...args),
  onP2PPeerDiscoveryChanged: vi.fn((cb: (event: unknown) => void) => {
    capturedDiscoveryCb = cb
    return Promise.resolve(mockDiscoveryUnlisten)
  }),
  onP2PPeerConnectionChanged: vi.fn((_cb: (event: unknown) => void) => {
    return Promise.resolve(mockConnectionUnlisten)
  }),
  onP2PPeerNameUpdated: vi.fn((_cb: (event: unknown) => void) => {
    return Promise.resolve(mockNameUnlisten)
  }),
}))

// NO mock for sonner -- hook should not import it at all

describe('useDeviceDiscovery', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockGetP2PPeers.mockResolvedValue([])
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('Test 1: initial load calls getP2PPeers and sets up three event listeners when active=true', async () => {
    const { unmount } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(mockGetP2PPeers).toHaveBeenCalledTimes(1)
    })

    // Three event listener registrations should have happened
    expect(p2pApi.onP2PPeerDiscoveryChanged).toHaveBeenCalledTimes(1)
    expect(p2pApi.onP2PPeerConnectionChanged).toHaveBeenCalledTimes(1)
    expect(p2pApi.onP2PPeerNameUpdated).toHaveBeenCalledTimes(1)

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

    // After fetch resolves, should have devices
    await waitFor(() => {
      expect(result.current.scanPhase).toBe('hasDevices')
    })

    expect(result.current.peers).toHaveLength(1)
    expect(result.current.peers[0].id).toBe('peer-1')
    expect(result.current.peers[0].deviceName).toBe('MacBook Pro')
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

  it('Test 4: discovery event with discovered=true adds peer and transitions to hasDevices', async () => {
    mockGetP2PPeers.mockResolvedValue([])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    // Wait for listeners to be registered
    await waitFor(() => {
      expect(p2pApi.onP2PPeerDiscoveryChanged).toHaveBeenCalledTimes(1)
    })

    expect(result.current.scanPhase).toBe('scanning')

    act(() => {
      capturedDiscoveryCb({
        peerId: 'peer-2',
        deviceName: 'iPhone',
        addresses: [],
        discovered: true,
      })
    })

    expect(result.current.peers).toHaveLength(1)
    expect(result.current.peers[0].id).toBe('peer-2')
    expect(result.current.peers[0].deviceName).toBe('iPhone')
    expect(result.current.scanPhase).toBe('hasDevices')
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
      capturedDiscoveryCb({
        peerId: 'peer-3',
        deviceName: 'Windows PC',
        addresses: [],
        discovered: true,
      })
    })

    expect(result.current.scanPhase).toBe('hasDevices')
    expect(result.current.peers).toHaveLength(1)
  })

  it('Test 6: resetScan clears peers, resets to scanning, starts fresh 10s timeout, re-fetches peers', async () => {
    vi.useFakeTimers()
    mockGetP2PPeers.mockResolvedValue([
      { peerId: 'peer-1', deviceName: 'MacBook', addresses: [], isPaired: false, connected: false },
    ])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    // Allow initial load to resolve
    await act(async () => {
      await vi.runAllTimersAsync()
    })

    expect(result.current.peers).toHaveLength(1)

    // After resetScan, second call returns empty list
    mockGetP2PPeers.mockResolvedValue([])

    // Reset
    act(() => {
      result.current.resetScan()
    })

    // Should be reset to scanning with empty peers immediately
    expect(result.current.peers).toHaveLength(0)
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

  it('Test 7: when active goes false then true, state resets (peers=[], scanPhase=scanning) before re-setup', async () => {
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
      expect(result.current.peers).toHaveLength(1)
    })

    // Deactivate
    rerender({ active: false })

    // Should reset to empty state
    expect(result.current.peers).toHaveLength(0)
    expect(result.current.scanPhase).toBe('scanning')

    // Re-activate
    mockGetP2PPeers.mockResolvedValue([])
    rerender({ active: true })

    // Should start fresh in scanning phase
    expect(result.current.scanPhase).toBe('scanning')
    expect(result.current.peers).toHaveLength(0)
  })

  it('Test 8: getP2PPeers() rejection logs error via console.error, calls onError callback, hook remains in scanning phase', async () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const onError = vi.fn()
    mockGetP2PPeers.mockRejectedValueOnce(new Error('network'))

    const { result } = renderHook(() => useDeviceDiscovery(true, { onError }))

    await waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalled()
    })

    expect(result.current.scanPhase).toBe('scanning')
    expect(result.current.peers).toHaveLength(0)
    expect(onError).toHaveBeenCalledWith(expect.any(Error))

    consoleErrorSpy.mockRestore()
  })

  it('Test 9: cleanup on unmount calls all three unlisten functions and clears timeout', async () => {
    mockGetP2PPeers.mockResolvedValue([])

    const { unmount } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(p2pApi.onP2PPeerDiscoveryChanged).toHaveBeenCalledTimes(1)
    })

    unmount()

    // Wait for async cleanup (unlisten promises to resolve)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(mockDiscoveryUnlisten).toHaveBeenCalledTimes(1)
    expect(mockConnectionUnlisten).toHaveBeenCalledTimes(1)
    expect(mockNameUnlisten).toHaveBeenCalledTimes(1)
  })

  it('Test 10: anonymous peer from getP2PPeers has deviceName: null (hook stores raw value, no fallback)', async () => {
    mockGetP2PPeers.mockResolvedValue([
      { peerId: 'peer-anon', deviceName: null, addresses: [], isPaired: false, connected: false },
    ])

    const { result } = renderHook(() => useDeviceDiscovery(true))

    await waitFor(() => {
      expect(result.current.peers).toHaveLength(1)
    })

    // deviceName should be null (raw from backend), NOT a fallback string
    expect(result.current.peers[0].deviceName).toBeNull()
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
})
