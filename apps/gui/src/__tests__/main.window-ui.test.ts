import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => {
  const render = vi.fn()
  return {
    applyPlatformEffectPreferences: vi.fn(),
    applyDiagnosticDeviceContext: vi.fn(),
    connectDaemonWs: vi.fn(() => Promise.resolve()),
    createRoot: vi.fn(() => ({ render })),
    getDeviceMeta: vi.fn(() => Promise.resolve({})),
    initializeDiagnostics: vi.fn(),
    initializeWindowUi: vi.fn(),
    initializeWindowTheme: vi.fn(() => Promise.resolve()),
    registerDaemonShutdownListener: vi.fn(() => Promise.resolve()),
    render,
  }
})

vi.mock('react-dom/client', () => ({
  default: { createRoot: mocks.createRoot },
  createRoot: mocks.createRoot,
}))

vi.mock('@/api/runtime', () => ({
  getDeviceMeta: mocks.getDeviceMeta,
}))

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: mocks.connectDaemonWs,
  registerDaemonShutdownListener: mocks.registerDaemonShutdownListener,
}))

vi.mock('@/lib/window-ui', () => ({
  applyPlatformEffectPreferences: mocks.applyPlatformEffectPreferences,
  initializeWindowUi: mocks.initializeWindowUi,
}))

vi.mock('@/lib/window-theme', () => ({ initializeWindowTheme: mocks.initializeWindowTheme }))
vi.mock('@/quick-panel/QuickPanelApp', () => ({ default: () => null }))

vi.mock('@/observability/diagnostics', () => ({
  applyDiagnosticDeviceContext: mocks.applyDiagnosticDeviceContext,
  initializeDiagnostics: mocks.initializeDiagnostics,
  DiagnosticsErrorBoundary: ({ children }: { children: ReactNode }) => children,
}))

// The early startup screen has its own test; these tests cover the bootstrap module.
vi.mock('@/startup-screen', () => ({ showStartupScreen: vi.fn() }))
vi.mock('@/store', () => ({
  store: {},
}))

vi.mock('@/App', () => ({
  default: () => null,
}))

describe('main window bootstrap', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
    document.body.innerHTML = '<div id="root"></div>'
  })

  it('启动主窗口时应用已保存的窗口 UI 设置', async () => {
    await import('@/main')
    await vi.dynamicImportSettled()

    expect(mocks.initializeWindowUi).toHaveBeenCalledTimes(1)
    expect(mocks.createRoot).toHaveBeenCalledWith(document.getElementById('root'))
    expect(mocks.initializeWindowUi.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.createRoot.mock.invocationCallOrder[0]!
    )
  })
})

it('waits for the desktop palette before mounting the quick panel', async () => {
  vi.resetModules()
  vi.clearAllMocks()
  document.body.innerHTML = '<div id="root"></div>'
  let finish!: () => void
  mocks.initializeWindowTheme.mockReturnValueOnce(
    new Promise<void>(resolve => {
      finish = resolve
    })
  )
  await import('@/quick-panel/main')
  expect(mocks.createRoot).not.toHaveBeenCalled()
  finish()
  await Promise.resolve()
  expect(mocks.createRoot).toHaveBeenCalledWith(document.getElementById('root'))
})
