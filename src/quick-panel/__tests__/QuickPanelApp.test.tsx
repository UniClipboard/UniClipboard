import { act, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings } from '@/api/daemon'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import type { DesktopTheme } from '@/lib/desktop-theme'
import { themePresets } from '@/lib/theme-engine'
import QuickPanelApp from '../QuickPanelApp'

const connectDaemonWsMock = vi.fn()
const panelRenderMock = vi.fn()
const invokeMock = vi.fn()
let eventHandlers: Record<string, () => void> = {}

vi.mock('@/api/daemon', () => ({
  getSettings: vi.fn().mockResolvedValue({ general: { theme: 'system' } }),
}))
vi.mock('@/lib/desktop-theme', () => ({ subscribeDesktopTheme: vi.fn(() => () => {}) }))

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
  afterEach(() => vi.restoreAllMocks())
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(subscribeDesktopTheme).mockImplementation(() => () => {})
    eventHandlers = {}
    document.documentElement.removeAttribute('style')
    document.documentElement.classList.remove('dark', 'light')
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

  it('themes the connecting screen before daemon bootstrap finishes', async () => {
    const pendingBootstrap = deferred()
    connectDaemonWsMock.mockReturnValue(pendingBootstrap.promise)
    let receive!: (theme: DesktopTheme | null) => void
    const unsubscribe = vi.fn()
    vi.mocked(subscribeDesktopTheme).mockImplementation(callback => {
      receive = callback
      return unsubscribe
    })
    const { unmount } = render(<QuickPanelApp />)
    expect(screen.getByText('Connecting clipboard history...')).toBeInTheDocument()
    expect(getSettings).not.toHaveBeenCalled()
    expect(document.documentElement.style.getPropertyValue('--card')).not.toBe('')
    expect(subscribeDesktopTheme).toHaveBeenCalledOnce()
    act(() => receive({ dark: true, variables: { '--card': '#343f44' } }))
    expect(document.documentElement).toHaveClass('dark')
    expect(document.documentElement.style.getPropertyValue('--card')).toBe('#343f44')
    pendingBootstrap.resolve()
    await waitFor(() => expect(getSettings).toHaveBeenCalledOnce())
    expect(subscribeDesktopTheme).toHaveBeenCalledOnce()
    expect(document.documentElement.style.getPropertyValue('--card')).toBe('#343f44')
    unmount()
    expect(unsubscribe).toHaveBeenCalledOnce()
  })

  it.each(['dark', 'light'] as const)('uses the system %s palette on the first render', mode => {
    const query = window.matchMedia('(prefers-color-scheme: dark)')
    vi.spyOn(window, 'matchMedia').mockReturnValue({ ...query, matches: mode === 'dark' })
    connectDaemonWsMock.mockReturnValue(deferred().promise)
    render(<QuickPanelApp />)
    expect(screen.getByText('Connecting clipboard history...')).toBeInTheDocument()
    expect(document.documentElement).toHaveClass(mode)
    expect(document.documentElement.style.getPropertyValue('--card')).toBe(
      themePresets.zinc[mode].card
    )
    expect(getSettings).not.toHaveBeenCalled()
  })

  it('announces readiness after subscribing to show requests', async () => {
    connectDaemonWsMock.mockReturnValue(deferred().promise)

    render(<QuickPanelApp />)

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'mark_quick_panel_ready',
        expect.objectContaining({ trace: expect.any(Object) })
      )
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
