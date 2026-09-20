import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AppContentWithBar } from '@/App'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'
import type { DaemonStartupStatus } from '@/lib/ipc'
import { WINDOW_FRAME_STORAGE_KEY } from '@/lib/window-frame'

const state = vi.hoisted(() => ({
  platform: { isWindows: true, isLinux: false, isMac: false, isTauri: true },
  retrying: false,
  failed: true,
  connected: false,
  hydrated: false,
  setupRequired: false,
  locked: false,
  recovering: false,
  checkingEncryption: false,
  autoUnlockEnabled: true,
  settingsLoading: false,
  spaceReadiness: 'ready' as 'ready' | 'recoveringMembership',
  presentation: vi.fn(),
  setEncryptionStatus: vi.fn(),
  startupStatus: null as DaemonStartupStatus | null,
  window: {
    minimize: vi.fn().mockResolvedValue(undefined),
    maximize: vi.fn().mockResolvedValue(undefined),
    unmaximize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
  },
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => state.window,
}))
vi.mock('@/hooks/usePlatform', () => ({ usePlatform: () => state.platform }))
vi.mock('@/hooks/useProfileRecovery', () => ({
  useProfileRecovery: () => ({
    status: { backgroundReady: !state.recovering },
    failed: false,
    refresh: vi.fn(),
  }),
}))
vi.mock('@/hooks/useContentUnlocked', async () => {
  const { useState } = await import('react')
  return {
    useContentUnlocked: () => {
      const [verified, setVerified] = useState(false)
      return {
        unlocked: state.settingsLoading ? null : verified || state.autoUnlockEnabled,
        refresh: () => setVerified(true),
      }
    },
  }
})
vi.mock('@/components', async () => import('@/components/TitleBar'))
vi.mock('@/layouts', async () => import('@/layouts/WindowShell'))
vi.mock('@/lib/logger', () => ({
  createLogger: () => ({ debug: vi.fn(), error: vi.fn() }),
}))
vi.mock('@/lib/ipc', () => ({
  commands: {
    setTrafficLightPosition: vi.fn().mockResolvedValue(undefined),
    unlockContentFromKeyring: vi.fn().mockResolvedValue(true),
  },
}))
vi.mock('@/contexts/SettingContext', () => ({ SettingProvider: () => null }))
vi.mock('@/contexts/UpdateContext', () => ({ UpdateProvider: () => null }))
vi.mock('@/contexts/SearchContext', () => ({ SearchProvider: () => null }))
vi.mock('@/contexts/ShortcutContext', () => ({
  ShortcutProvider: ({ children }: { children: ReactNode }) => children,
}))
vi.mock('@/components/motion/VisualEffectsProvider', () => ({
  default: () => null,
}))
vi.mock('@/hooks/useUINavigateListener', () => ({
  useUINavigateListener: vi.fn(),
}))
vi.mock('@/hooks/useVisualEffectsSampling', () => ({
  useVisualEffectsSampling: vi.fn(),
}))
vi.mock('@/hooks/useMainWindowPresentation', () => ({
  useMainWindowPresentation: (ready: boolean) => state.presentation(ready),
}))
vi.mock('@/hooks/useSetting', () => ({
  useSettingSelector: (
    selector: (context: {
      setting: { security: { autoUnlockEnabled: boolean } } | null
      loading: boolean
      error: string | null
    }) => unknown
  ) =>
    selector({
      setting: { security: { autoUnlockEnabled: state.autoUnlockEnabled } },
      loading: state.settingsLoading,
      error: null,
    }),
}))
vi.mock('react-router', () => ({
  BrowserRouter: () => null,
  useNavigate: () => vi.fn(),
}))
vi.mock('@/store/setupRealtimeStore', () => ({
  useSetupRealtimeStore: () => ({
    hydrated: state.hydrated,
    flow: !state.hydrated
      ? { kind: 'loading' }
      : state.setupRequired
        ? { kind: 'entry' }
        : { kind: 'completed', deviceName: 'test', completion: null },
  }),
}))
vi.mock('@/hooks/useAppBootstrap', () => ({
  useAppBootstrap: () => ({
    bootstrapFailure: state.failed ? { kind: 'spawnFailed', detail: 'offline' } : null,
    retrying: state.retrying,
    daemonBootstrapReady: state.connected,
    startupStatus: state.startupStatus,
    resolvedEncryptionStatus: state.checkingEncryption
      ? null
      : { initialized: true, session_ready: !state.locked },
    spaceReadiness: state.spaceReadiness,
    setEncryptionStatus: state.setEncryptionStatus,
    retry: vi.fn(),
  }),
}))
vi.mock('@/components/app/AppStatusScreen', () => ({
  AppStatusScreen: () => <main>Startup failure</main>,
}))
vi.mock('@/components/app/AuthenticatedRoutes', () => ({
  AuthenticatedRoutes: () => <main>History</main>,
}))
vi.mock('@/pages/SetupPage', () => ({
  default: ({ onCompleteSetup }: { onCompleteSetup?: () => void }) => (
    <main>
      Setup
      <button type="button" onClick={onCompleteSetup}>
        Complete setup
      </button>
    </main>
  ),
}))
vi.mock('@/pages/ProfileRecoveryPage', () => ({ default: () => <main>Recover local data</main> }))
vi.mock('@/pages/UnlockPage', () => ({
  default: ({ onUnlockSucceeded }: { onUnlockSucceeded?: () => void }) => (
    <main>
      <button type="button" onClick={onUnlockSucceeded}>
        Unlock
      </button>
    </main>
  ),
}))
vi.mock('@/components/ui/toaster', () => ({ Toaster: () => null }))

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.removeItem(WINDOW_FRAME_STORAGE_KEY)
  delete window.__UC_WINDOW_FRAME_DEFAULT__
  state.retrying = false
  state.failed = true
  state.connected = false
  state.hydrated = false
  state.setupRequired = false
  state.locked = false
  state.recovering = false
  state.checkingEncryption = false
  state.autoUnlockEnabled = true
  state.settingsLoading = false
  state.spaceReadiness = 'ready'
  state.startupStatus = null
  state.platform = {
    isWindows: true,
    isLinux: false,
    isMac: false,
    isTauri: true,
  }
})
afterEach(cleanup)

describe('startup window frame before setup hydration', () => {
  it('updates the encryption state when setup completes', () => {
    state.connected = true
    state.failed = false
    state.hydrated = true
    state.setupRequired = true
    state.checkingEncryption = true
    render(<AppContentWithBar />)

    fireEvent.click(screen.getByRole('button', { name: 'Complete setup' }))

    expect(state.setEncryptionStatus).toHaveBeenCalledWith({
      initialized: true,
      session_ready: true,
    })
  })

  it('updates both encryption and content access after a manual unlock', () => {
    state.connected = true
    state.failed = false
    state.hydrated = true
    state.locked = true
    state.autoUnlockEnabled = false
    render(<AppContentWithBar />)

    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }))

    expect(state.setEncryptionStatus).toHaveBeenCalledWith({
      initialized: true,
      session_ready: true,
    })
  })

  it('shows recovery before setup and settings can be hydrated', () => {
    state.connected = true
    state.failed = false
    state.hydrated = false
    state.checkingEncryption = true
    state.recovering = true
    render(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent('Recover local data')
    expect(screen.queryByText('Setup')).toBeNull()
    expect(state.presentation).toHaveBeenLastCalledWith(true)
  })
  it.each([false, true])('hides titlebar on a tiling desktop while retrying=%s', retrying => {
    state.platform = {
      isWindows: false,
      isLinux: true,
      isMac: false,
      isTauri: true,
    }
    window.__UC_WINDOW_FRAME_DEFAULT__ = 'none'
    state.retrying = retrying
    state.failed = !retrying
    const { container } = render(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent(
      retrying ? 'Starting the app' : 'Startup failure'
    )
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
    expect(container.querySelector('[data-tauri-drag-region]')).toBeNull()
  })

  it('preserves an explicit custom frame on a tiling desktop before setup', () => {
    state.platform = {
      isWindows: false,
      isLinux: true,
      isMac: false,
      isTauri: true,
    }
    window.__UC_WINDOW_FRAME_DEFAULT__ = 'none'
    localStorage.setItem(WINDOW_FRAME_STORAGE_KEY, 'false')
    render(<AppContentWithBar />)
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })

  it('does not reveal a historical upgrade snapshot while reopening a ready daemon', () => {
    state.failed = false
    state.startupStatus = {
      package_version: '1.0.0',
      service_ready: true,
      service_failed: false,
      progress: makeUpgradePreview('ready', 43),
    }
    render(<AppContentWithBar />)
    expect(state.presentation).toHaveBeenLastCalledWith(false)
    expect(screen.queryByText(/资料已升级|Data upgraded/)).not.toBeInTheDocument()
  })

  it('reveals a genuinely running upgrade without waiting for restored content', () => {
    state.failed = false
    state.startupStatus = {
      package_version: '1.0.0',
      service_ready: false,
      service_failed: false,
      progress: makeUpgradePreview('upgrading', 20),
    }
    render(<AppContentWithBar />)
    expect(state.presentation).toHaveBeenLastCalledWith(true)
  })
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
    state.platform = {
      isWindows: false,
      isLinux: false,
      isMac: true,
      isTauri: true,
    }
    const { container } = render(<AppContentWithBar />)
    expect(container.querySelector('[data-tauri-drag-region="true"]')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })

  it('leaves setup page chrome to the setup layout', () => {
    state.failed = false
    state.connected = true
    state.hydrated = true
    state.setupRequired = true
    render(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent('Setup')
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })

  it('keeps one startup screen until an existing user can enter history', () => {
    state.failed = false
    const view = render(<AppContentWithBar />)
    expect(screen.getByRole('heading')).toHaveTextContent(/正在启动|Starting the app/)
    expect(screen.queryByText('Setup')).not.toBeInTheDocument()
    expect(state.presentation).toHaveBeenLastCalledWith(false)
    state.connected = true
    view.rerender(<AppContentWithBar />)
    expect(screen.getByRole('heading')).toHaveTextContent(/正在启动|Starting the app/)
    state.hydrated = true
    view.rerender(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent('History')
    expect(state.presentation).toHaveBeenLastCalledWith(true)
    expect(screen.queryByText('Setup')).not.toBeInTheDocument()
  })

  it('keeps the active view stretched across the window', () => {
    const { container } = render(<AppContentWithBar />)
    const transitionRoot = container.querySelector('.relative.h-full.overflow-hidden')
    const activeView = transitionRoot?.firstElementChild

    expect(transitionRoot).toHaveClass('w-full')
    expect(activeView).toHaveClass('flex')
    expect(activeView).not.toHaveClass('flex-col')
  })

  it('opens the restricted app while space membership awaits user recovery', () => {
    state.failed = false
    state.connected = true
    state.hydrated = true
    state.spaceReadiness = 'recoveringMembership'

    const view = render(<AppContentWithBar />)

    expect(screen.getByRole('main')).toHaveTextContent('History')
    expect(state.presentation).toHaveBeenLastCalledWith(true)

    state.spaceReadiness = 'ready'
    view.rerender(<AppContentWithBar />)

    expect(screen.getByRole('main')).toHaveTextContent('History')
  })

  it('opens the wizard only after setup is confirmed necessary', () => {
    state.failed = false
    state.connected = true
    const view = render(<AppContentWithBar />)
    expect(screen.queryByText('Setup')).not.toBeInTheDocument()
    state.hydrated = true
    state.setupRequired = true
    view.rerender(<AppContentWithBar />)
    expect(screen.getByRole('main')).toHaveTextContent('Setup')
  })

  it('keeps content hidden when auto unlock is disabled even though the engine is ready', async () => {
    state.failed = false
    state.connected = true
    state.hydrated = true
    state.checkingEncryption = true
    state.autoUnlockEnabled = false
    const view = render(<AppContentWithBar />)
    expect(screen.getByRole('heading')).toHaveTextContent(/正在启动|Starting the app/)
    state.checkingEncryption = false
    view.rerender(<AppContentWithBar />)
    await waitFor(() => expect(screen.getByText('Unlock')).toBeInTheDocument())
  })

  it('uses the same page background behind the toolbar for startup and unlock', async () => {
    const view = render(<AppContentWithBar />)
    expect(screen.getByRole('main').closest('.bg-background')).not.toBeNull()
    view.unmount()

    state.failed = false
    state.connected = true
    state.hydrated = true
    state.autoUnlockEnabled = false
    render(<AppContentWithBar />)

    const unlock = await screen.findByText('Unlock')
    expect(unlock.closest('main')?.closest('.bg-background')).not.toBeNull()
  })

  it('reveals content after the content lock authenticates without changing engine readiness', async () => {
    state.failed = false
    state.connected = true
    state.hydrated = true
    state.autoUnlockEnabled = false

    render(<AppContentWithBar />)

    const unlock = await screen.findByRole('button', { name: 'Unlock' })
    fireEvent.click(unlock)

    expect(screen.getByRole('main')).toHaveTextContent('History')
  })

  it('offers manual unlock when automatic recovery has no usable key', async () => {
    state.failed = false
    state.connected = true
    state.hydrated = true
    state.autoUnlockEnabled = true
    state.locked = true
    render(<AppContentWithBar />)
    expect(await screen.findByRole('button', { name: 'Unlock' })).toBeVisible()
    expect(screen.queryByText('History')).not.toBeInTheDocument()
  })
})
