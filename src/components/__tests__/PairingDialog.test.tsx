// @vitest-environment jsdom
import { render, screen, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import PairingDialog from '@/components/PairingDialog'
import { PairingNotificationProvider } from '@/components/PairingNotificationProvider'
import PairingPinDialog from '@/components/PairingPinDialog'

// Module-level mock refs — created in a hoisted scope so vi.mock factories can access them.
const {
  getP2PPeersMock,
  initiateP2PPairingMock,
  verifyP2PPairingPinMock,
  acceptP2PPairingMock,
  rejectP2PPairingMock,
  toastMock,
} = vi.hoisted(() => {
  const toastFn = vi.fn() as ReturnType<typeof vi.fn> & {
    error: ReturnType<typeof vi.fn>
    success: ReturnType<typeof vi.fn>
  }
  toastFn.error = vi.fn()
  toastFn.success = vi.fn()

  return {
    getP2PPeersMock: vi.fn(),
    initiateP2PPairingMock: vi.fn(),
    verifyP2PPairingPinMock: vi.fn(),
    acceptP2PPairingMock: vi.fn(),
    rejectP2PPairingMock: vi.fn(),
    toastMock: toastFn,
  }
})

// Capture handlers registered via daemonWs.subscribe for test injection
const capturedHandlers = {
  pairing: null as
    | ((event: { topic: string; eventType: string; payload: Record<string, unknown> }) => void)
    | null,
  setup: null as
    | ((event: { topic: string; eventType: string; payload: Record<string, unknown> }) => void)
    | null,
}
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((topics: string[], handler: (event: unknown) => void) => {
      if (topics.includes('pairing'))
        capturedHandlers.pairing = handler as typeof capturedHandlers.pairing
      if (topics.includes('setup'))
        capturedHandlers.setup = handler as typeof capturedHandlers.setup
      return vi.fn()
    }),
  },
}))

vi.mock('@/api/daemon/pairing', () => ({
  getP2PPeers: (...args: unknown[]) => getP2PPeersMock(...args),
  initiateP2PPairing: (...args: unknown[]) => initiateP2PPairingMock(...args),
  verifyP2PPairingPin: (...args: unknown[]) => verifyP2PPairingPinMock(...args),
  acceptP2PPairing: (...args: unknown[]) => acceptP2PPairingMock(...args),
  rejectP2PPairing: (...args: unknown[]) => rejectP2PPairingMock(...args),
  getPairedPeers: vi.fn(),
  getPairedPeersWithStatus: vi.fn(),
  getLocalDeviceInfo: vi.fn(),
  unpairP2PDevice: vi.fn(),
}))

vi.mock('@/api/daemon/events', () => ({
  classifyPairingError: (error?: string | null) => {
    const normalized = error?.toLowerCase() ?? ''
    if (normalized.includes('active pairing session exists')) return 'active_session_exists'
    if (
      normalized.includes('pairing session not found') ||
      normalized.includes('session_not_found')
    )
      return 'session_not_found'
    if (normalized.includes('connection refused') || normalized.includes('daemon connection info'))
      return 'daemon_unavailable'
    return 'unknown'
  },
}))

vi.mock('sonner', () => ({
  toast: toastMock,
}))

/** Helper: emit a synthetic pairing WS event into the captured handler */
function emitPairingEvent(payload: Record<string, unknown>) {
  const handler = capturedHandlers.pairing
  if (!handler) throw new Error('No pairing handler registered')
  act(() => {
    handler({ topic: 'pairing', eventType: 'pairing.verification_required', payload })
  })
}

function emitPairingUpdated(state: string, payload: Record<string, unknown> = {}) {
  const handler = capturedHandlers.pairing
  if (!handler) throw new Error('No pairing handler registered')
  act(() => {
    handler({ topic: 'pairing', eventType: 'pairing.updated', payload: { state, ...payload } })
  })
}

function emitPairingComplete(payload: Record<string, unknown> = {}) {
  const handler = capturedHandlers.pairing
  if (!handler) throw new Error('No pairing handler registered')
  act(() => {
    handler({ topic: 'pairing', eventType: 'pairing.complete', payload })
  })
}

function emitPairingFailed(reason?: string, extra: Record<string, unknown> = {}) {
  const handler = capturedHandlers.pairing
  if (!handler) throw new Error('No pairing handler registered')
  act(() => {
    handler({ topic: 'pairing', eventType: 'pairing.failed', payload: { reason, ...extra } })
  })
}

describe('PairingDialog', () => {
  beforeEach(() => {
    getP2PPeersMock.mockResolvedValue([])
    initiateP2PPairingMock.mockResolvedValue({ success: true, sessionId: 'session-1' })
    verifyP2PPairingPinMock.mockResolvedValue(undefined)
    capturedHandlers.pairing = null
    capturedHandlers.setup = null
    toastMock.mockClear()
    toastMock.error.mockClear()
    toastMock.success.mockClear()
  })

  it('shows loading state after confirming PIN match', async () => {
    render(<PairingDialog open onClose={vi.fn()} />)
    const user = userEvent.setup()

    await act(async () => {})

    emitPairingEvent({ sessionId: 'session-1', code: '123456', state: 'verification' })

    const confirmButton = await screen.findByRole('button', {
      name: /确认匹配|Confirm Match/i,
    })

    await user.click(confirmButton)

    expect(verifyP2PPairingPinMock).toHaveBeenCalledWith('session-1', true)
    expect(confirmButton).toBeDisabled()
    expect(confirmButton).toHaveTextContent(/正在验证|Verifying/i)
  })

  it('keeps initiator flow on the active session until completion', async () => {
    render(<PairingDialog open onClose={vi.fn()} />)
    const user = userEvent.setup()

    await act(async () => {})

    emitPairingEvent({ sessionId: 'session-1', code: '123456' })
    emitPairingUpdated('verification', { sessionId: 'session-1' })

    const confirmButton = await screen.findByRole('button', {
      name: /确认匹配|Confirm Match/i,
    })
    await user.click(confirmButton)

    emitPairingUpdated('verifying', { sessionId: 'other-session' })
    expect(confirmButton).toHaveTextContent(/正在验证|Verifying/i)

    emitPairingComplete({ sessionId: 'other-session', deviceName: 'Other' })
    expect(screen.queryByText(/配对成功|Pairing Successful/i)).not.toBeInTheDocument()

    emitPairingComplete({ sessionId: 'session-1', deviceName: 'PeerB' })

    expect(await screen.findAllByText(/配对成功|Pairing Successful/i)).toHaveLength(2)
  })

  it('shows localized failure only for the active initiator session', async () => {
    render(<PairingDialog open onClose={vi.fn()} />)

    await act(async () => {})

    emitPairingEvent({ sessionId: 'session-1', code: '123456' })

    expect(await screen.findByText('123456')).toBeInTheDocument()

    emitPairingFailed('pairing session not found', { sessionId: 'other-session' })
    expect(screen.queryByText(/配对失败|Pairing Failed/i)).not.toBeInTheDocument()

    emitPairingFailed('pairing session not found', { sessionId: 'session-1' })

    expect(await screen.findAllByText(/配对失败|Pairing Failed/i)).toHaveLength(2)
    expect(
      await screen.findAllByText(
        /配对会话已过期或已关闭|The pairing session expired or was already closed/i
      )
    ).toHaveLength(2)
  })
})

describe('PairingDialog failure states', () => {
  beforeEach(() => {
    capturedHandlers.pairing = null
    capturedHandlers.setup = null
    toastMock.mockClear()
    toastMock.error.mockClear()
    toastMock.success.mockClear()
    getP2PPeersMock.mockResolvedValue([
      {
        peerId: 'peer-1',
        deviceName: 'Desk',
        addresses: [],
        isPaired: false,
        connected: true,
      },
    ])
  })

  it('shows localized active session error for initiator failures', async () => {
    const user = userEvent.setup()
    initiateP2PPairingMock.mockResolvedValue({
      success: false,
      sessionId: '',
      error: 'active pairing session exists',
    })

    render(<PairingDialog open onClose={vi.fn()} />)

    await act(async () => {})
    await user.click(screen.getByText('Desk').closest('button')!)

    expect(
      await screen.findAllByText(
        /已有正在进行的配对，请稍后再试|Another pairing session is already in progress/i
      )
    ).toHaveLength(2)
  })

  it('shows localized session expired error for missing sessions', async () => {
    const user = userEvent.setup()
    initiateP2PPairingMock.mockResolvedValue({
      success: false,
      sessionId: '',
      error: 'pairing session not found',
    })

    render(<PairingDialog open onClose={vi.fn()} />)

    await act(async () => {})
    await user.click(screen.getByText('Desk').closest('button')!)

    expect(
      await screen.findAllByText(
        /配对会话已过期或已关闭|The pairing session expired or was already closed/i
      )
    ).toHaveLength(2)
  })

  it('shows localized daemon unavailable error when initiate throws', async () => {
    const user = userEvent.setup()
    initiateP2PPairingMock.mockRejectedValue(
      new Error('failed to call daemon pairing route /pairing/initiate: connection refused')
    )

    render(<PairingDialog open onClose={vi.fn()} />)

    await act(async () => {})
    await user.click(screen.getByText('Desk').closest('button')!)

    expect(
      await screen.findAllByText(
        /配对 daemon 不可用，请启动桌面服务后重试|The pairing daemon is unavailable. Start the desktop service and try again/i
      )
    ).toHaveLength(2)
  })
})

describe('PairingPinDialog', () => {
  it('does not cancel when clicking outside the modal', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()

    render(
      <PairingPinDialog
        open
        onClose={onClose}
        pinCode="123456"
        peerDeviceName="Desk"
        isInitiator={false}
        onConfirm={vi.fn()}
        phase="display"
      />
    )

    const overlay = document.querySelector('[data-slot="dialog-overlay"]')
    expect(overlay).not.toBeNull()

    await user.click(overlay as Element)

    expect(onClose).not.toHaveBeenCalled()
  })
})

describe('PairingNotificationProvider — accept->verification race regression', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    capturedHandlers.pairing = null
    capturedHandlers.setup = null
    acceptP2PPairingMock.mockResolvedValue(undefined)
    rejectP2PPairingMock.mockResolvedValue(undefined)
    toastMock.mockClear()
    toastMock.error.mockClear()
    toastMock.success.mockClear()
  })

  it('verification event immediately after accept is not dropped — PIN dialog appears', async () => {
    render(<PairingNotificationProvider />)

    await act(async () => {})

    expect(capturedHandlers.pairing).not.toBeNull()

    // Step 1: backend sends a pairing request (pairing.updated with state: 'request')
    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'request',
          sessionId: 'session-abc',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    expect(toastMock).toHaveBeenCalled()
    const toastCall = toastMock.mock.calls[0]
    const toastOptions = toastCall[1] as { action?: { onClick?: () => void } }
    expect(toastOptions.action?.onClick).toBeDefined()

    // Step 2: user clicks Accept.
    act(() => {
      toastOptions.action!.onClick!()
    })

    // Step 3: backend immediately pushes verification for the same session
    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.verification_required',
        payload: {
          sessionId: 'session-abc',
          code: '123456',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    await screen.findByText('123456')
    expect(screen.getByText('123456')).toBeInTheDocument()
  })

  it('accept failure rolls back session — subsequent verification is ignored', async () => {
    acceptP2PPairingMock.mockRejectedValue(new Error('accept failed'))

    render(<PairingNotificationProvider />)

    await act(async () => {})

    expect(capturedHandlers.pairing).not.toBeNull()

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'request',
          sessionId: 'session-fail',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    const toastOptions = toastMock.mock.calls[0][1] as { action?: { onClick?: () => void } }

    act(() => {
      toastOptions.action!.onClick!()
    })

    await act(async () => {})

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.verification_required',
        payload: {
          sessionId: 'session-fail',
          code: '999999',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    expect(screen.queryByText('999999')).not.toBeInTheDocument()
  })

  it('keeps verifying state when verification_required repeats with kind=verifying', async () => {
    render(<PairingNotificationProvider />)

    await act(async () => {})

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'request',
          sessionId: 'session-verifying',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    const toastOptions = toastMock.mock.calls[0][1] as { action?: { onClick?: () => void } }

    act(() => {
      toastOptions.action!.onClick!()
    })

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.verification_required',
        payload: {
          sessionId: 'session-verifying',
          kind: 'verification',
          code: '123456',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    await screen.findByText('123456')

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'verifying',
          sessionId: 'session-verifying',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    expect(await screen.findAllByText(/正在验证|Verifying/i)).toHaveLength(2)
    expect(screen.queryByText('123456')).not.toBeInTheDocument()

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.verification_required',
        payload: {
          sessionId: 'session-verifying',
          kind: 'verifying',
          deviceName: 'PeerB',
          peerId: 'peer-id-b',
        },
      })
    })

    expect(await screen.findAllByText(/正在验证|Verifying/i)).toHaveLength(2)
    expect(screen.queryByText('123456')).not.toBeInTheDocument()
  })
})

describe('PairingNotificationProvider — session-aware provider diagnostics', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    capturedHandlers.pairing = null
    capturedHandlers.setup = null
    acceptP2PPairingMock.mockResolvedValue(undefined)
    rejectP2PPairingMock.mockResolvedValue(undefined)
    toastMock.mockClear()
    toastMock.error.mockClear()
    toastMock.success.mockClear()
  })

  it('logs accepted when user clicks Accept on a pairing request', async () => {
    const debugSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})

    render(<PairingNotificationProvider />)
    await act(async () => {})

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'request',
          sessionId: 'session-diag',
          deviceName: 'PeerB',
          peerId: 'peer-b',
        },
      })
    })

    const toastOptions = toastMock.mock.calls[0][1] as { action?: { onClick?: () => void } }
    act(() => {
      toastOptions.action!.onClick!()
    })

    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('accepted'))
    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('session-diag'))
    debugSpy.mockRestore()
  })

  it('logs ignored with session_mismatch when verification arrives for a different session', async () => {
    const debugSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})

    render(<PairingNotificationProvider />)
    await act(async () => {})

    // Accept session-active
    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'request',
          sessionId: 'session-active',
          deviceName: 'PeerA',
          peerId: 'peer-a',
        },
      })
    })
    const toastOptions = toastMock.mock.calls[0][1] as { action?: { onClick?: () => void } }
    act(() => {
      toastOptions.action!.onClick!()
    })

    // Verification for a different session
    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.verification_required',
        payload: {
          sessionId: 'session-other',
          code: '999999',
          deviceName: 'PeerB',
          peerId: 'peer-b',
        },
      })
    })

    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('ignored'))
    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('session_mismatch'))
    expect(screen.queryByText('999999')).not.toBeInTheDocument()
    debugSpy.mockRestore()
  })

  it('logs rejected when user dismisses the request toast', async () => {
    const debugSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})

    render(<PairingNotificationProvider />)
    await act(async () => {})

    act(() => {
      capturedHandlers.pairing!({
        topic: 'pairing',
        eventType: 'pairing.updated',
        payload: {
          state: 'request',
          sessionId: 'session-reject',
          deviceName: 'PeerC',
          peerId: 'peer-c',
        },
      })
    })

    const toastOptions = toastMock.mock.calls[0][1] as { cancel?: { onClick?: () => void } }
    act(() => {
      toastOptions.cancel!.onClick!()
    })

    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('rejected'))
    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('session-reject'))
    debugSpy.mockRestore()
  })
})
