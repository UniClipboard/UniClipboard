import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { updateDebugMode } from '@/api/daemon/diagnostics'
import type { UpdateMetadata } from '@/api/updater'
import Sidebar from '@/components/layout/Sidebar'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateContext, type UpdateContextType, type UpdateState } from '@/contexts/update-context'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { Settings } from '@/types/setting'

vi.mock('@/api/daemon/diagnostics', () => ({
  updateDebugMode: vi.fn(),
}))

vi.mock('framer-motion', async () => {
  const ReactModule = await import('react')
  return {
    m: {
      div: ({
        children,
        layoutId,
        ...props
      }: React.HTMLAttributes<HTMLDivElement> & { layoutId?: string }) =>
        ReactModule.createElement('div', { ...props, 'data-layout-id': layoutId }, children),
    },
  }
})

const mockUpdateDebugMode = vi.mocked(updateDebugMode)

beforeAll(() => {
  if ('ResizeObserver' in globalThis) return
  Object.defineProperty(globalThis, 'ResizeObserver', {
    configurable: true,
    value: class ResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  })
})

const baseSetting: Settings = makeBaseSettings()

const updateInfo: UpdateMetadata = {
  version: '0.1.1',
  currentVersion: '0.1.0',
  date: '2026-01-25T00:00:00Z',
  body: 'Bug fixes',
}

function buildUpdateValue(state: UpdateState): UpdateContextType {
  return {
    state,
    updateInfo: state.info,
    downloadProgress: {
      phase: state.phase,
      downloaded: state.downloaded,
      total: state.total,
    },
    isCheckingUpdate: false,
    checkForUpdates: vi.fn().mockResolvedValue(null),
    downloadUpdate: vi.fn().mockResolvedValue(undefined),
    cancelDownload: vi.fn().mockResolvedValue(undefined),
    installUpdate: vi.fn().mockResolvedValue(undefined),
    installKind: 'macos',
    isSystemManaged: false,
    isManualUpdate: false,
  }
}

function renderSidebar(state: UpdateState, setting: Settings = baseSetting) {
  const reloadSetting = vi.fn().mockResolvedValue(undefined)
  return render(
    <SettingContext.Provider
      value={{
        setting,
        loading: false,
        error: null,
        reloadSetting,
        updateSetting: vi.fn(),
        updateGeneralSetting: vi.fn(),
        updateAutostart: vi.fn(),
        updateSyncSetting: vi.fn(),
        updateSecuritySetting: vi.fn(),
        updateRetentionPolicy: vi.fn(),
        updateKeyboardShortcuts: vi.fn(),
        updateFileSyncSetting: vi.fn(),
        updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
        updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
      }}
    >
      <UpdateContext.Provider value={buildUpdateValue(state)}>
        <MemoryRouter>
          <Sidebar />
        </MemoryRouter>
      </UpdateContext.Provider>
    </SettingContext.Provider>
  )
}

function renderSidebarAt(pathname: string) {
  return render(
    <SettingContext.Provider
      value={{
        setting: baseSetting,
        loading: false,
        error: null,
        reloadSetting: vi.fn(),
        updateSetting: vi.fn(),
        updateGeneralSetting: vi.fn(),
        updateAutostart: vi.fn(),
        updateSyncSetting: vi.fn(),
        updateSecuritySetting: vi.fn(),
        updateRetentionPolicy: vi.fn(),
        updateKeyboardShortcuts: vi.fn(),
        updateFileSyncSetting: vi.fn(),
        updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
        updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
      }}
    >
      <UpdateContext.Provider
        value={buildUpdateValue({ phase: 'idle', info: null, downloaded: 0, total: null })}
      >
        <MemoryRouter initialEntries={[pathname]}>
          <Sidebar />
        </MemoryRouter>
      </UpdateContext.Provider>
    </SettingContext.Provider>
  )
}

describe('Sidebar update indicator', () => {
  it('keeps the primary navigation highlight inside the active item', () => {
    renderSidebarAt('/devices')

    const activeLink = document.querySelector('a[href="/devices"]')
    const activeHighlight = activeLink?.querySelector(':scope > div[aria-hidden]')
    expect(activeHighlight).toHaveClass('absolute', 'inset-0', 'rounded-lg', 'bg-primary/10')
    expect(activeHighlight).not.toHaveClass('transition-transform', 'will-change-transform')
  })

  it('keeps inactive navigation hover backgrounds rounded without transparent clipping', () => {
    renderSidebarAt('/devices')

    expect(document.querySelector('aside')).toHaveClass('bg-transparent')

    const historyLink = document.querySelector('a[href="/history"]')
    expect(historyLink).toHaveClass('rounded-lg', 'hover:bg-muted')
    expect(historyLink).not.toHaveClass('overflow-hidden', 'transition-colors')
    expect(historyLink?.firstElementChild).not.toHaveClass('transition-colors')
  })

  it('keeps navigation tooltips inside the transparent sidebar layer', async () => {
    const user = userEvent.setup()
    renderSidebarAt('/devices')

    const sidebar = document.querySelector('aside')
    const historyLink = document.querySelector('a[href="/history"]')
    expect(sidebar).toBeInTheDocument()
    expect(historyLink).toBeInTheDocument()

    await user.hover(historyLink!)

    await waitFor(() => {
      expect(sidebar?.querySelector('[data-slot="tooltip-content"]')).toBeInTheDocument()
    })
  })

  it('shows the amber "available" icon when an update is available', async () => {
    renderSidebar({
      phase: 'available',
      info: updateInfo,
      downloaded: 0,
      total: null,
    })

    const button = await waitFor(() => screen.getByLabelText(/update available/i))
    expect(button).toHaveAttribute('data-update-state', 'available')
  })

  it('hides the icon when there is no update', () => {
    renderSidebar({ phase: 'idle', info: null, downloaded: 0, total: null })

    expect(screen.queryByLabelText(/update available/i)).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/downloading update/i)).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/update ready/i)).not.toBeInTheDocument()
  })

  it('shows the "downloading" indicator with progress text when downloading', async () => {
    renderSidebar({
      phase: 'downloading',
      info: updateInfo,
      downloaded: 512,
      total: 1024,
    })

    const button = await waitFor(() => screen.getByLabelText(/downloading update.*50/i))
    expect(button).toHaveAttribute('data-update-state', 'downloading')
  })

  it('shows the "downloading" indicator without percent when total is unknown', async () => {
    renderSidebar({
      phase: 'downloading',
      info: updateInfo,
      downloaded: 512,
      total: null,
    })

    const button = await waitFor(() => screen.getByLabelText(/downloading update/i))
    expect(button).toHaveAttribute('data-update-state', 'downloading')
    expect(button).not.toHaveTextContent(/%/)
  })

  it('shows the emerald "ready" indicator when the update has been downloaded', async () => {
    renderSidebar({
      phase: 'ready',
      info: updateInfo,
      downloaded: 1024,
      total: 1024,
    })

    const button = await waitFor(() => screen.getByLabelText(/update ready/i))
    expect(button).toHaveAttribute('data-update-state', 'ready')
  })

  it('shows debug badge and disables debug mode through diagnostics', async () => {
    mockUpdateDebugMode.mockResolvedValue({ debugMode: false, restartRequired: true })
    const debugSetting: Settings = {
      ...baseSetting,
      general: { ...baseSetting.general, debugMode: true },
    }

    renderSidebar({ phase: 'idle', info: null, downloaded: 0, total: null }, debugSetting)

    await userEvent.click(screen.getByRole('button', { name: 'Turn off Debug mode' }))

    await waitFor(() => {
      expect(mockUpdateDebugMode).toHaveBeenCalledWith(false)
    })
  })
})
