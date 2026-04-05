// @vitest-environment jsdom
// Tests for event-driven device discovery replacing the old 3-second polling approach
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { daemonWs } from '@/lib/daemon-ws'
import SetupPage from '@/pages/SetupPage'

// Module-level mock refs — reset in beforeEach
const getP2PPeersMock = vi.fn()
const selectJoinPeerMock = vi.fn()
const runActionMock = vi.fn()
const setSelectedPeerIdMock = vi.fn()

vi.mock('@/hooks/useSetupFlow', () => ({
  useSetupFlow: () => ({
    setupState: { JoinSpaceSelectDevice: { error: null } },
    hydrated: true,
    stepInfo: null,
    direction: 'forward' as const,
    loading: false,
    runAction: runActionMock,
    selectedPeerId: null,
    setSelectedPeerId: setSelectedPeerIdMock,
  }),
}))

vi.mock('@/hooks/useDeviceDiscovery', () => ({
  useDeviceDiscovery: vi.fn(() => ({ scanPhase: 'scanning' as const, resetScan: vi.fn() })),
}))

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

vi.mock('@/api/daemon/setup', () => ({
  startNewSpace: vi.fn(),
  startJoinSpace: vi.fn(),
  selectJoinPeer: (...args: unknown[]) => selectJoinPeerMock(...args),
  submitPassphrase: vi.fn(),
  verifyPassphrase: vi.fn(),
  cancelSetup: vi.fn(),
  confirmPeerTrust: vi.fn(),
}))

// Capture daemonWs.subscribe handlers for test injection
const capturedWsHandlers: Array<
  (event: { topic: string; eventType: string; payload: unknown }) => void
> = []
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((_topics: string[], handler: (typeof capturedWsHandlers)[number]) => {
      capturedWsHandlers.push(handler)
      return vi.fn(() => {
        const idx = capturedWsHandlers.indexOf(handler)
        if (idx !== -1) capturedWsHandlers.splice(idx, 1)
      })
    }),
  },
}))

// Mock Redux store
const dispatchMock = vi.fn()
vi.mock('react-redux', () => ({
  useSelector: vi.fn(() => []),
  useDispatch: () => dispatchMock,
}))

const navigateMock = vi.fn()
const translationFnByPrefix = new Map<string, (key: string) => string>()
vi.mock('react-router-dom', () => ({
  useNavigate: () => navigateMock,
}))

vi.mock('react-i18next', () => ({
  useTranslation: (_ns?: string, opts?: { keyPrefix?: string }) => {
    const keyPrefix = opts?.keyPrefix ?? ''
    if (!translationFnByPrefix.has(keyPrefix)) {
      translationFnByPrefix.set(keyPrefix, (key: string) =>
        keyPrefix ? `${keyPrefix}.${key}` : key
      )
    }

    return {
      t: translationFnByPrefix.get(keyPrefix)!,
    }
  },
}))

vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get: () => (props: HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

describe('setup event-driven device discovery', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    capturedWsHandlers.length = 0
    getP2PPeersMock.mockReset()
    getP2PPeersMock.mockResolvedValue([])
    selectJoinPeerMock.mockReset()
    runActionMock.mockReset()
    setSelectedPeerIdMock.mockReset()
    dispatchMock.mockReset()
    navigateMock.mockReset()
    translationFnByPrefix.clear()
  })

  afterEach(() => {
    cleanup()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('calls getP2PPeers on mount and sets up daemonWs event listeners', async () => {
    render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(getP2PPeersMock).toHaveBeenCalled()
    })

    // daemonWs.subscribe should have been called for peers topic
    expect(daemonWs.subscribe).toHaveBeenCalled()

    const callsBeforeAdvance = getP2PPeersMock.mock.calls.length

    // Advance 6 seconds — NO repeated polling should occur
    await act(async () => {
      vi.advanceTimersByTime(6000)
    })

    expect(getP2PPeersMock.mock.calls.length).toBe(callsBeforeAdvance)
  })

  it('shows scanning state then transitions to empty after timeout', async () => {
    getP2PPeersMock.mockResolvedValue([])

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(getP2PPeersMock).toHaveBeenCalled()
    })

    // Scanning state should be visible initially
    expect(view.getByText('setup.joinPickDevice.scanning.title')).toBeTruthy()

    // After 10 seconds, empty state should appear
    await act(async () => {
      vi.advanceTimersByTime(10000)
    })

    expect(view.getByText('setup.joinPickDevice.empty.title')).toBeTruthy()
  })

  it('discovery event adds device to list via Redux', async () => {
    // Simulate a peers.changed event being pushed via daemonWs.subscribe
    const handler = capturedWsHandlers[0]

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(handler).toBeTruthy()
    })

    // Simulate peers.changed event from daemon
    act(() => {
      handler({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [
            {
              peerId: 'peer-1',
              deviceName: 'Test Device',
              connected: false,
            },
          ],
        },
      })
    })

    // Device card should appear with the device name
    expect(view.getByText('Test Device')).toBeTruthy()
  })

  it('selects a discovered device and advances join pairing progression', async () => {
    selectJoinPeerMock.mockResolvedValue({
      ProcessingJoinSpace: { message: 'waiting for pairing verification' },
    })

    const handler = capturedWsHandlers[0]

    render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(handler).toBeTruthy()
    })

    act(() => {
      handler({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [
            {
              peerId: 'peer-join-1',
              deviceName: 'Pairing Host',
              connected: false,
            },
          ],
        },
      })
    })

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'setup.joinPickDevice.actions.select' }))
    })

    expect(selectJoinPeerMock).toHaveBeenCalledWith('peer-join-1')
  })

  it('cleans up event listeners on unmount', async () => {
    const unsubscribeFn = vi.fn()
    ;(daemonWs.subscribe as ReturnType<typeof vi.fn>).mockReturnValue(unsubscribeFn)

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(daemonWs.subscribe).toHaveBeenCalled()
    })

    // Unmount the component
    view.unmount()
    await act(async () => {})

    // Cleanup function should have been called
    expect(unsubscribeFn).toHaveBeenCalled()
  })

  it('anonymous device renders with i18n fallback from render layer', async () => {
    const handler = capturedWsHandlers[0]

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(handler).toBeTruthy()
    })

    act(() => {
      handler({
        topic: 'peers',
        eventType: 'peers.changed',
        payload: {
          peers: [
            {
              peerId: 'peer-anon',
              deviceName: null,
              connected: false,
            },
          ],
        },
      })
    })

    // The render layer applies tCommon('unknownDevice') fallback.
    expect(view.getByText('setup.common.unknownDevice')).toBeTruthy()
  })
})
