import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import QuickPanelApp from '../QuickPanelApp'

const connectDaemonWsMock = vi.fn()
const panelRenderMock = vi.fn()

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: (...args: unknown[]) => connectDaemonWsMock(...args),
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
})
