import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings } from '@/api/daemon'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import type { DesktopTheme } from '@/lib/desktop-theme'
import { resetSettingsSyncForTest } from '@/lib/settings-sync'
import { DEFAULT_THEME_COLOR, themePresets } from '@/lib/theme-engine'
import { connectDaemonWsForTest, disconnectDaemonWsForTest } from '@/test/fixtures/settings-ws'
import QuickPanelApp from '../QuickPanelApp'

const connectDaemonWsMock = vi.fn()
const panelRenderMock = vi.fn()
const invokeMock = vi.fn()
let eventHandlers: Record<string, () => void> = {}
const contentAccess = vi.hoisted(() => ({ unlocked: true as boolean | null, refresh: vi.fn() }))
vi.mock('@/hooks/useContentUnlocked', () => ({
  useContentUnlocked: () => ({ unlocked: contentAccess.unlocked, refresh: contentAccess.refresh }),
}))

vi.mock('@/store/setupRealtimeStore', () => ({
  ensureSetupRealtimeSync: vi.fn().mockResolvedValue(undefined),
  refreshSetupState: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/ui-kit', () => ({
  UiRoot: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children: ReactNode }) => <p>{children}</p>,
  Button: ({
    children,
    onClick,
    busy,
    submitForm,
  }: {
    children: ReactNode
    onClick?: () => void
    busy?: boolean
    submitForm?: string
  }) => (
    <button
      type={submitForm ? 'submit' : 'button'}
      form={submitForm}
      disabled={busy}
      onClick={onClick}
    >
      {children}
    </button>
  ),
  FormField: ({
    id,
    label,
    value,
    onValueChange,
    error,
  }: {
    id: string
    label: string
    value: string
    onValueChange: (value: string) => void
    error?: string
  }) => (
    <label>
      {label}
      <input
        id={id}
        type="password"
        value={value}
        onChange={event => onValueChange(event.target.value)}
      />
      {error && <span role="alert">{error}</span>}
    </label>
  ),
}))

vi.mock('@/api/daemon', () => ({
  getSettings: vi.fn().mockResolvedValue({ general: { theme: 'system' } }),
}))
vi.mock('@/lib/desktop-theme', () => ({ subscribeDesktopTheme: vi.fn(() => () => {}) }))

vi.mock('@/lib/daemon-ws', () => import('@/test/fixtures/settings-ws'))
vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  // The settings snapshot arrives once the socket is open.
  connectDaemonWs: (...args: unknown[]) =>
    Promise.resolve(connectDaemonWsMock(...args)).then(value => {
      connectDaemonWsForTest()
      return value
    }),
}))

vi.mock('@host/hostservice', () => {
  // Each generated binding reports its call to `invokeMock`, so the tests observe what crosses to the Go service.
  const call =
    (name: string) =>
    (...args: unknown[]) =>
      invokeMock(name, ...args)
  return {
    BeginVisualEffectsSample: call('BeginVisualEffectsSample'),
    FinalizeQuickPanelShow: call('FinalizeQuickPanelShow'),
    GetVisualEffects: call('GetVisualEffects'),
    MarkQuickPanelReady: call('MarkQuickPanelReady'),
    ReportVisualEffectsEnvironment: call('ReportVisualEffectsEnvironment'),
    ReportVisualEffectsSample: call('ReportVisualEffectsSample'),
    SetQuickPanelLayout: call('SetQuickPanelLayout'),
    SetVisualEffectsMode: call('SetVisualEffectsMode'),
    UnlockContentFromKeyring: call('UnlockContentFromKeyring'),
    UnlockContent: call('UnlockContent'),
    DismissQuickPanel: call('DismissQuickPanel'),
  }
})

vi.mock('@/host/event', () => ({
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
    resetSettingsSyncForTest()
    disconnectDaemonWsForTest()
    contentAccess.unlocked = true
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

  it.each([false, null])('does not mount history or previews when access is %s', async unlocked => {
    contentAccess.unlocked = unlocked
    connectDaemonWsMock.mockResolvedValue(undefined)
    const view = render(<QuickPanelApp />)
    await waitFor(() => expect(connectDaemonWsMock).toHaveBeenCalled())
    expect(panelRenderMock).not.toHaveBeenCalled()
    expect(screen.queryByText('Clipboard history panel')).not.toBeInTheDocument()
    contentAccess.unlocked = true
    view.rerender(<QuickPanelApp />)
    expect(screen.getByText('Clipboard history panel')).toBeVisible()
  })

  it('unlocks from the keyring inside the panel without opening the main window', async () => {
    contentAccess.unlocked = false
    connectDaemonWsMock.mockResolvedValue(undefined)
    invokeMock.mockImplementation((command: string) =>
      Promise.resolve(command === 'UnlockContentFromKeyring')
    )
    render(<QuickPanelApp />)
    fireEvent.click(await screen.findByRole('button', { name: 'Unlock' }))
    await waitFor(() => expect(contentAccess.refresh).toHaveBeenCalledOnce())
    expect(invokeMock).toHaveBeenCalledWith('UnlockContentFromKeyring')
    expect(invokeMock).not.toHaveBeenCalledWith('ShowContentUnlock')
  })

  it('accepts a passphrase in the panel, keeps wrong-passphrase content hidden and clears it on reopen', async () => {
    contentAccess.unlocked = false
    connectDaemonWsMock.mockResolvedValue(undefined)
    invokeMock.mockImplementation((command: string) =>
      command === 'UnlockContent'
        ? Promise.reject({ code: 'WRONG_PASSPHRASE' })
        : Promise.resolve(false)
    )
    render(<QuickPanelApp />)
    fireEvent.click(await screen.findByRole('button', { name: 'Unlock' }))
    const input = await screen.findByLabelText('Passphrase')
    fireEvent.change(input, { target: { value: 'wrong' } })
    fireEvent.submit(input.closest('form')!)
    expect(await screen.findByRole('alert')).toBeVisible()
    expect(contentAccess.refresh).not.toHaveBeenCalled()
    expect(panelRenderMock).not.toHaveBeenCalled()
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('DismissQuickPanel'))
    await act(async () => eventHandlers['quick-panel://prepare-show']?.())
    expect(screen.queryByLabelText('Passphrase')).not.toBeInTheDocument()
    fireEvent.click(await screen.findByRole('button', { name: 'Unlock' }))
    const nextInput = await screen.findByLabelText('Passphrase')
    expect(nextInput).toHaveValue('')
    invokeMock.mockResolvedValue(undefined)
    fireEvent.change(nextInput, { target: { value: 'correct' } })
    fireEvent.submit(nextInput.closest('form')!)
    await waitFor(() => expect(contentAccess.refresh).toHaveBeenCalledOnce())
    expect(invokeMock).not.toHaveBeenCalledWith('ShowContentUnlock')
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
      themePresets[DEFAULT_THEME_COLOR][mode].card
    )
    expect(getSettings).not.toHaveBeenCalled()
  })

  it('announces readiness after subscribing to show requests', async () => {
    connectDaemonWsMock.mockReturnValue(deferred().promise)

    render(<QuickPanelApp />)

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('MarkQuickPanelReady')
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
      expect(invokeMock).toHaveBeenCalledWith('FinalizeQuickPanelShow')
    })
    expect(panelRenderMock).not.toHaveBeenCalled()
  })
})
