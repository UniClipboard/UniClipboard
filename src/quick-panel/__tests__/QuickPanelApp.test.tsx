import { act, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import QuickPanelApp from '../QuickPanelApp'

const connectDaemonWsMock = vi.fn()
const panelRenderMock = vi.fn()
const invokeMock = vi.fn()
let eventHandlers: Record<string, () => void> = {}

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: (...args: unknown[]) => connectDaemonWsMock(...args),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: () => void) => {
    eventHandlers[event] = handler
    return Promise.resolve(() => {
      delete eventHandlers[event]
    })
  }),
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    initialized: false,
  },
}))

vi.mock('../ClipboardHistoryPanel', () => ({
  default: () => {
    panelRenderMock()
    return <div>Clipboard history panel</div>
  },
}))

function deferred() {
  let resolve!: () => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<void>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('QuickPanelApp', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    eventHandlers = {}
    invokeMock.mockResolvedValue(undefined)
  })

  it('waits for daemon bootstrap before mounting the clipboard panel', async () => {
    const pendingBootstrap = deferred()
    connectDaemonWsMock.mockReturnValue(pendingBootstrap.promise)

    render(<QuickPanelApp />)

    expect(screen.getByText('Connecting clipboard history...')).toBeInTheDocument()
    expect(panelRenderMock).not.toHaveBeenCalled()

    pendingBootstrap.resolve()

    await waitFor(() => {
      expect(screen.getByText('Clipboard history panel')).toBeInTheDocument()
    })
  })

  it('finalizes show events even while daemon bootstrap is still pending', async () => {
    const pendingBootstrap = deferred()
    connectDaemonWsMock.mockReturnValue(pendingBootstrap.promise)

    render(<QuickPanelApp />)

    expect(screen.getByText('Connecting clipboard history...')).toBeInTheDocument()

    await act(async () => {
      eventHandlers['quick-panel://prepare-show']?.()
      await Promise.resolve()
    })

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'finalize_quick_panel_show',
        expect.objectContaining({ trace: expect.any(Object) })
      )
    })
    expect(panelRenderMock).not.toHaveBeenCalled()
  })
})
