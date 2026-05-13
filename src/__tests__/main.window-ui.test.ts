import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => {
  const render = vi.fn()
  return {
    applyPlatformEffectPreferences: vi.fn(),
    applyPlatformTypographyScale: vi.fn(),
    applyDeviceMetaToSentry: vi.fn(),
    attachConsole: vi.fn(() => Promise.resolve()),
    connectDaemonWs: vi.fn(() => Promise.resolve()),
    createRoot: vi.fn(() => ({ render })),
    getDeviceMeta: vi.fn(() => Promise.resolve({})),
    initSentry: vi.fn(),
    initializeWindowUi: vi.fn(),
    registerDaemonShutdownListener: vi.fn(() => Promise.resolve()),
    render,
  }
})

vi.mock('react-dom/client', () => ({
  default: { createRoot: mocks.createRoot },
  createRoot: mocks.createRoot,
}))

vi.mock('@tauri-apps/plugin-log', () => ({
  attachConsole: mocks.attachConsole,
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
  applyPlatformTypographyScale: mocks.applyPlatformTypographyScale,
  initializeWindowUi: mocks.initializeWindowUi,
}))

vi.mock('@/observability/sentry', () => ({
  applyDeviceMetaToSentry: mocks.applyDeviceMetaToSentry,
  initSentry: mocks.initSentry,
  Sentry: {
    ErrorBoundary: ({ children }: { children: ReactNode }) => children,
  },
}))

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

    expect(mocks.initializeWindowUi).toHaveBeenCalledTimes(1)
    expect(mocks.createRoot).toHaveBeenCalledWith(document.getElementById('root'))
    expect(mocks.initializeWindowUi.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.createRoot.mock.invocationCallOrder[0]!
    )
  })
})
