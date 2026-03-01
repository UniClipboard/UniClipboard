// @vitest-environment jsdom
import { act, cleanup, render } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { getP2PPeers } from '@/api/p2p'
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

describe('setup peer discovery polling', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    ;(getSetupState as Mock).mockReset()
    ;(getP2PPeers as Mock).mockReset()
    ;(selectJoinPeer as Mock).mockReset()
    navigateMock.mockReset()
    ;(getSetupState as Mock).mockResolvedValue({ JoinSpaceSelectDevice: { error: null } })
    ;(getP2PPeers as Mock).mockResolvedValue([])
  })

  afterEach(() => {
    cleanup()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('starts polling after entering JoinSpaceSelectDevice', async () => {
    render(<SetupPage />)
    await act(async () => {})

    await vi.waitFor(() => {
      expect(getP2PPeers).toHaveBeenCalled()
    })
    const initialCalls = (getP2PPeers as Mock).mock.calls.length

    await act(async () => {
      vi.advanceTimersByTime(6000)
    })

    expect((getP2PPeers as Mock).mock.calls.length).toBeGreaterThanOrEqual(initialCalls + 2)
  })

  it('stops polling after leaving JoinSpaceSelectDevice', async () => {
    ;(getP2PPeers as Mock).mockResolvedValue([
      {
        peerId: 'peer-1',
        deviceName: 'Peer One',
        addresses: [],
        isPaired: false,
        connected: true,
      },
    ])
    ;(selectJoinPeer as Mock).mockResolvedValue({ JoinSpaceInputPassphrase: { error: null } })

    const view = render(<SetupPage />)
    await act(async () => {})
    await vi.waitFor(() => {
      expect(getP2PPeers).toHaveBeenCalled()
      expect(
        view.getByRole('button', {
          name: 'setup.joinPickDevice.actions.select',
        })
      ).toBeTruthy()
    })

    const selectButton = view.getByRole('button', {
      name: 'setup.joinPickDevice.actions.select',
    }) as HTMLButtonElement
    await act(async () => {
      selectButton.click()
    })

    const callsAfterLeave = (getP2PPeers as Mock).mock.calls.length

    await act(async () => {
      vi.advanceTimersByTime(6000)
    })

    expect((getP2PPeers as Mock).mock.calls.length).toBe(callsAfterLeave)
  })

  it('restores loading state when getP2PPeers fails', async () => {
    const rejectRefreshRef: { current: ((reason?: unknown) => void) | null } = { current: null }
    ;(getP2PPeers as Mock)
      .mockResolvedValueOnce([])
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, reject) => {
            rejectRefreshRef.current = reject
          })
      )
      .mockResolvedValueOnce([])

    const view = render(<SetupPage />)
    await act(async () => {})
    await vi.waitFor(() => {
      expect(getP2PPeers).toHaveBeenCalled()
    })

    const refreshButton = view.getByRole('button', { name: 'setup.common.refresh' })
    await act(async () => {
      refreshButton.click()
    })

    await vi.waitFor(() => {
      expect((getP2PPeers as Mock).mock.calls.length).toBeGreaterThanOrEqual(2)
    })

    rejectRefreshRef.current?.(new Error('network failure'))
    await act(async () => {})

    await vi.waitFor(() => {
      expect((refreshButton as HTMLButtonElement).disabled).toBe(false)
    })
  })
})
