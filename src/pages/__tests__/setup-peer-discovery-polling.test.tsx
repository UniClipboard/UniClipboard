// @vitest-environment jsdom
// Tests for event-driven device discovery replacing the old 3-second polling approach
import { act, cleanup, render } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import {
  getP2PPeers,
  onP2PPeerConnectionChanged,
  onP2PPeerDiscoveryChanged,
  onP2PPeerNameUpdated,
} from '@/api/p2p'
import { getSetupState, selectJoinPeer } from '@/api/setup'
import SetupPage from '@/pages/SetupPage'

vi.mock('@/api/setup', () => ({
  getSetupState: vi.fn(),
  onSetupStateChanged: vi.fn(() => Promise.resolve(() => {})),
  startNewSpace: vi.fn(),
  startJoinSpace: vi.fn(),
  selectJoinPeer: vi.fn(),
  submitPassphrase: vi.fn(),
  verifyPassphrase: vi.fn(),
  cancelSetup: vi.fn(),
  confirmPeerTrust: vi.fn(),
}))

vi.mock('@/api/p2p', () => ({
  getP2PPeers: vi.fn(),
  onP2PPeerDiscoveryChanged: vi.fn(() => Promise.resolve(() => {})),
  onP2PPeerConnectionChanged: vi.fn(() => Promise.resolve(() => {})),
  onP2PPeerNameUpdated: vi.fn(() => Promise.resolve(() => {})),
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
    ;(getSetupState as Mock).mockReset()
    ;(getP2PPeers as Mock).mockReset()
    ;(selectJoinPeer as Mock).mockReset()
    ;(onP2PPeerDiscoveryChanged as Mock).mockReset()
    ;(onP2PPeerConnectionChanged as Mock).mockReset()
    ;(onP2PPeerNameUpdated as Mock).mockReset()
    navigateMock.mockReset()
    ;(getSetupState as Mock).mockResolvedValue({ JoinSpaceSelectDevice: { error: null } })
    ;(getP2PPeers as Mock).mockResolvedValue([])
    ;(onP2PPeerDiscoveryChanged as Mock).mockResolvedValue(() => {})
    ;(onP2PPeerConnectionChanged as Mock).mockResolvedValue(() => {})
    ;(onP2PPeerNameUpdated as Mock).mockResolvedValue(() => {})
  })

  afterEach(() => {
    cleanup()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('calls getP2PPeers on mount and sets up event listeners', async () => {
    render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(getP2PPeers).toHaveBeenCalled()
    })

    // Event listeners must be set up
    expect(onP2PPeerDiscoveryChanged).toHaveBeenCalledTimes(1)
    expect(onP2PPeerConnectionChanged).toHaveBeenCalledTimes(1)
    expect(onP2PPeerNameUpdated).toHaveBeenCalledTimes(1)

    const callsBeforeAdvance = (getP2PPeers as Mock).mock.calls.length

    // Advance 6 seconds -- NO repeated polling should occur
    await act(async () => {
      vi.advanceTimersByTime(6000)
    })

    expect((getP2PPeers as Mock).mock.calls.length).toBe(callsBeforeAdvance)
  })

  it('shows scanning state then transitions to empty after timeout', async () => {
    ;(getP2PPeers as Mock).mockResolvedValue([])

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(getP2PPeers).toHaveBeenCalled()
    })

    // Scanning state should be visible initially
    expect(view.getByText('setup.joinPickDevice.scanning.title')).toBeTruthy()

    // After 10 seconds, empty state should appear
    await act(async () => {
      vi.advanceTimersByTime(10000)
    })

    expect(view.getByText('setup.joinPickDevice.empty.title')).toBeTruthy()
  })

  it('discovery event adds device to list', async () => {
    let discoveryCallback:
      | ((event: {
          peerId: string
          deviceName: string | null
          addresses: string[]
          discovered: boolean
        }) => void)
      | null = null

    ;(onP2PPeerDiscoveryChanged as Mock).mockImplementation((cb: typeof discoveryCallback) => {
      discoveryCallback = cb
      return Promise.resolve(() => {})
    })

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(discoveryCallback).not.toBeNull()
    })

    // Fire discovery event with a device
    await act(async () => {
      discoveryCallback!({
        peerId: 'peer-1',
        deviceName: 'Test Device',
        addresses: [],
        discovered: true,
      })
    })

    // Device card should appear with the device name
    expect(view.getByText('Test Device')).toBeTruthy()
  })

  it('cleans up event listeners on unmount', async () => {
    const cleanupSpy = vi.fn()
    ;(onP2PPeerDiscoveryChanged as Mock).mockResolvedValue(cleanupSpy)

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(onP2PPeerDiscoveryChanged).toHaveBeenCalledTimes(1)
    })

    // Unmount the component
    view.unmount()
    await act(async () => {})

    // Cleanup function should have been called
    expect(cleanupSpy).toHaveBeenCalled()
  })

  it('anonymous device renders with i18n fallback from render layer', async () => {
    let discoveryCallback:
      | ((event: {
          peerId: string
          deviceName: string | null
          addresses: string[]
          discovered: boolean
        }) => void)
      | null = null

    ;(onP2PPeerDiscoveryChanged as Mock).mockImplementation((cb: typeof discoveryCallback) => {
      discoveryCallback = cb
      return Promise.resolve(() => {})
    })

    const view = render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(discoveryCallback).not.toBeNull()
    })

    // Fire discovery event with null deviceName (anonymous device)
    await act(async () => {
      discoveryCallback!({
        peerId: 'peer-anon',
        deviceName: null,
        addresses: ['addr'],
        discovered: true,
      })
    })

    // The render layer applies tCommon('unknownDevice') fallback.
    // The mock t function with keyPrefix 'setup.common' returns 'setup.common.unknownDevice'
    // NOT the hardcoded English string 'Unknown device'
    expect(view.getByText('setup.common.unknownDevice')).toBeTruthy()
  })
})
