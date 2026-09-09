import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AppContentWithBar } from '@/App'
import { WINDOW_FRAME_STORAGE_KEY } from '@/lib/window-frame'

const state = vi.hoisted(() => ({
  platform: { isWindows: true, isLinux: false, isMac: false, isTauri: true },
  retrying: false,
  failed: true,
  window: {
    minimize: vi.fn().mockResolvedValue(undefined),
    maximize: vi.fn().mockResolvedValue(undefined),
    unmaximize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
  },
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => state.window }))
vi.mock('@/hooks/usePlatform', () => ({ usePlatform: () => state.platform }))
vi.mock('@/components', async () => import('@/components/TitleBar'))
vi.mock('@/layouts', async () => import('@/layouts/WindowShell'))
vi.mock('@/api/security', () => ({ unlockEncryptionSession: vi.fn() }))
vi.mock('@/lib/logger', () => ({ createLogger: () => ({ debug: vi.fn(), error: vi.fn() }) }))
vi.mock('@/lib/ipc', () => ({
  commands: { setTrafficLightPosition: vi.fn().mockResolvedValue(undefined) },
}))
vi.mock('@/contexts/SettingContext', () => ({ SettingProvider: () => null }))
vi.mock('@/contexts/UpdateContext', () => ({ UpdateProvider: () => null }))
vi.mock('@/contexts/SearchContext', () => ({ SearchProvider: () => null }))
vi.mock('@/contexts/ShortcutContext', () => ({
  ShortcutProvider: ({ children }: { children: ReactNode }) => children,
}))
vi.mock('@/components/motion/VisualEffectsProvider', () => ({ default: () => null }))
vi.mock('@/hooks/useUINavigateListener', () => ({ useUINavigateListener: vi.fn() }))
vi.mock('@/hooks/useVisualEffectsSampling', () => ({ useVisualEffectsSampling: vi.fn() }))
vi.mock('react-router', () => ({ BrowserRouter: () => null, useNavigate: () => vi.fn() }))
vi.mock('@/store/setupRealtimeStore', () => ({
  useSetupRealtimeStore: () => ({ hydrated: false, flow: { kind: 'idle' } }),
}))
vi.mock('@/hooks/useAppBootstrap', () => ({
  useAppBootstrap: () => ({
    bootstrapFailure: state.failed ? { kind: 'spawnFailed', detail: 'offline' } : null,
    retrying: state.retrying,
    retry: vi.fn(),
  }),
}))
vi.mock('@/components/app/AppStatusScreen', () => ({
  AppStatusScreen: () => <main>Startup failure</main>,
}))
vi.mock('@/components/app/AuthenticatedRoutes', () => ({ AuthenticatedRoutes: () => null }))
vi.mock('@/pages/SetupPage', () => ({ default: () => <main>Setup</main> }))
vi.mock('@/pages/UnlockPage', () => ({ default: () => null }))
vi.mock('@/components/ui/toaster', () => ({ Toaster: () => null }))

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.removeItem(WINDOW_FRAME_STORAGE_KEY)
  state.retrying = false
  state.failed = true
  state.platform = { isWindows: true, isLinux: false, isMac: false, isTauri: true }
})
afterEach(cleanup)

describe('startup window frame before setup hydration', () => {
  it.each(['Windows', 'Linux'])('keeps drag regions and window controls on %s', async platform => {
    state.platform.isWindows = platform === 'Windows'
    state.platform.isLinux = platform === 'Linux'
    const { container } = render(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent('Startup failure')
    expect(container.querySelector('[data-tauri-drag-region="true"]')).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: '最小化' }))
    fireEvent.click(screen.getByRole('button', { name: '最大化' }))
    fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    await waitFor(() => {
      expect(state.window.minimize).toHaveBeenCalledOnce()
      expect(state.window.maximize).toHaveBeenCalledOnce()
      expect(state.window.close).toHaveBeenCalledOnce()
    })
    expect(screen.getByRole('button', { name: '关闭' })).toHaveAttribute(
      'data-tauri-drag-region',
      'false'
    )
  })

  it('keeps the title bar while retrying', () => {
    state.retrying = true
    state.failed = false
    render(<AppContentWithBar />)
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })

  it('does not duplicate native controls in system frame mode', () => {
    localStorage.setItem(WINDOW_FRAME_STORAGE_KEY, 'true')
    render(<AppContentWithBar />)
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })

  it('keeps the macOS drag region without custom window buttons', () => {
    state.platform = { isWindows: false, isLinux: false, isMac: true, isTauri: true }
    const { container } = render(<AppContentWithBar />)
    expect(container.querySelector('[data-tauri-drag-region="true"]')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })

  it('leaves setup page chrome to the setup layout', () => {
    state.failed = false
    render(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent('Setup')
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })
})
