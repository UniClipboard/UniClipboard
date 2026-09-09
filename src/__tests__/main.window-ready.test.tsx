import { act, screen } from '@testing-library/react'
import type { Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  setDecorations: vi.fn(),
  markReady: vi.fn(),
  roots: [] as Root[],
  failApp: false,
}))
vi.mock('react-dom/client', async importOriginal => {
  const actual = await importOriginal<typeof import('react-dom/client')>()
  return {
    default: {
      createRoot: (...args: Parameters<typeof actual.createRoot>) => {
        const root = actual.createRoot(...args)
        mocks.roots.push(root)
        return root
      },
    },
  }
})
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => mocks }))
vi.mock('@tauri-apps/plugin-log', () => ({ attachConsole: vi.fn() }))
vi.mock('@/lib/platform', () => ({
  detectPlatformInfo: () => ({
    isLinux: true,
    isWindows: false,
    isMac: false,
    isTauri: true,
  }),
}))
vi.mock('@/lib/ipc', () => ({ commands: { markMainWindowReady: mocks.markReady } }))
vi.mock('@/lib/logger', () => ({ createLogger: () => ({ error: vi.fn() }) }))
vi.mock('@/i18n', () => ({}))
vi.mock('@/api/runtime', () => ({ getDeviceMeta: vi.fn().mockResolvedValue({}) }))
vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
  registerDaemonShutdownListener: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/lib/webview-context-menu', () => ({ initializeWebviewContextMenu: vi.fn() }))
vi.mock('@/lib/window-ui', () => ({ initializeWindowUi: vi.fn() }))
vi.mock('@/lib/wdio-test-bridge', () => ({}))
vi.mock('@/observability/diagnostics', async () => {
  const { ErrorBoundary } = await import('@sentry/react')
  return {
    applyDiagnosticDeviceContext: vi.fn(),
    initializeDiagnostics: vi.fn(),
    DiagnosticsErrorBoundary: ErrorBoundary,
  }
})
vi.mock('react-redux', () => ({
  Provider: ({ children }: { children: React.ReactNode }) => children,
}))
vi.mock('@/store', () => ({ store: {} }))
vi.mock('@/App', () => ({
  default: () => {
    if (mocks.failApp) throw new Error('Initial render failed')
    return <main>Startup failure</main>
  },
}))

beforeEach(() => {
  vi.resetModules()
  vi.clearAllMocks()
  mocks.roots = []
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(async () => {
  await act(async () => {
    for (const root of mocks.roots) root.unmount()
  })
  document.body.replaceChildren()
  delete window.__UC_MAIN_WINDOW_GENERATION__
  vi.restoreAllMocks()
})

it.each([false, true])(
  'waits for decorations and committed content, render failure=%s',
  async failApp => {
    mocks.failApp = failApp
    const expectedContent = failApp ? 'Something went wrong.' : 'Startup failure'
    const notifications: Array<{ generation: string; content: string | null }> = []
    let finish!: () => void
    mocks.setDecorations.mockImplementation(
      () =>
        new Promise<void>(resolve => {
          finish = resolve
        })
    )
    mocks.markReady.mockImplementation(async (generation: string) => {
      notifications.push({
        generation,
        content: document.getElementById('root')?.textContent ?? null,
      })
    })
    window.__UC_MAIN_WINDOW_GENERATION__ = '7'
    const host = document.createElement('div')
    host.id = 'root'
    document.body.append(host)

    await act(async () => {
      await import('@/main')
    })
    window.dispatchEvent(new Event('load'))
    expect(host).toBeEmptyDOMElement()
    expect(mocks.markReady).not.toHaveBeenCalled()

    await act(async () => {
      finish()
    })
    expect(screen.getByText(expectedContent)).toBeVisible()
    expect(mocks.markReady).toHaveBeenCalledWith('7')
    for (const notification of notifications) {
      expect(notification).toEqual({ generation: '7', content: expectedContent })
    }
  }
)
