import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { WINDOW_FRAME_STORAGE_KEY } from '@/lib/window-frame'
import LibraryMainLayout from '../LibraryMainLayout'

const platformState = vi.hoisted(() => ({
  current: {
    isWindows: false,
    isMac: false,
    isLinux: false,
    isTauri: false,
  },
}))

const windowMocks = vi.hoisted(() => ({
  close: vi.fn().mockResolvedValue(undefined),
  isMaximized: vi.fn().mockResolvedValue(false),
  maximize: vi.fn().mockResolvedValue(undefined),
  minimize: vi.fn().mockResolvedValue(undefined),
  onResized: vi.fn().mockResolvedValue(() => {}),
  unmaximize: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/host/window', () => ({
  getCurrentWindow: () => windowMocks,
}))

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: () => platformState.current,
}))

const shortcuts = vi.hoisted(() => ({ handlers: new Map<string, () => void>() }))
vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: ({ id, handler }: { id?: string; handler: () => void }) => {
    if (id) shortcuts.handlers.set(id, handler)
  },
}))
const trafficLight = vi.hoisted(() => ({ set: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@/lib/ipc', () => ({
  commands: { setTrafficLightPosition: trafficLight.set },
}))

const renderLayout = () =>
  render(
    <MemoryRouter>
      <LibraryMainLayout>
        <div data-testid="content" />
      </LibraryMainLayout>
    </MemoryRouter>
  )

const PLATFORMS = {
  windows: { isWindows: true, isMac: false, isLinux: false, isTauri: true },
  linux: { isWindows: false, isMac: false, isLinux: true, isTauri: true },
  mac: { isWindows: false, isMac: true, isLinux: false, isTauri: true },
  browser: { isWindows: false, isMac: false, isLinux: false, isTauri: false },
} as const

const renderOn = (platform: keyof typeof PLATFORMS, frame?: 'custom' | 'system') => {
  platformState.current = PLATFORMS[platform]
  localStorage.clear()
  if (frame) localStorage.setItem(WINDOW_FRAME_STORAGE_KEY, frame)
  return renderLayout()
}

describe('LibraryMainLayout', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    windowMocks.isMaximized.mockResolvedValue(false)
  })
  afterEach(() => localStorage.clear())

  it.each(['windows', 'linux'] as const)(
    'shows working window controls over the corner with the app-drawn frame on %s',
    async platform => {
      renderOn(platform, 'custom')

      const overlay = screen.getByRole('button', { name: '关闭' }).closest('.absolute')
      expect(overlay).toHaveClass('right-0', 'top-0', 'z-50', 'h-10')
      // The band stays draggable; only the buttons opt out.
      expect(overlay).toHaveAttribute('data-tauri-drag-region', 'true')

      fireEvent.click(screen.getByRole('button', { name: '最小化' }))
      fireEvent.click(screen.getByRole('button', { name: '最大化' }))
      fireEvent.click(screen.getByRole('button', { name: '关闭' }))
      await waitFor(() => {
        expect(windowMocks.minimize).toHaveBeenCalledOnce()
        expect(windowMocks.maximize).toHaveBeenCalledOnce()
        expect(windowMocks.close).toHaveBeenCalledOnce()
      })
      windowMocks.isMaximized.mockResolvedValueOnce(true)
      fireEvent.click(screen.getByRole('button', { name: '还原' }))
      await waitFor(() => expect(windowMocks.unmaximize).toHaveBeenCalledOnce())
    }
  )

  it.each(['windows', 'linux'] as const)(
    'reserves the corner for page headers only while the controls are shown on %s',
    platform => {
      const custom = renderOn(platform, 'custom')
      const customMain = custom.container.querySelector('main') as HTMLElement
      expect(customMain.style.getPropertyValue('--window-controls-inset-x')).toBe('9rem')
      expect(customMain.style.getPropertyValue('--window-controls-inset-y')).toBe('2.5rem')
      custom.unmount()

      const system = renderOn(platform, 'system')
      const systemMain = system.container.querySelector('main') as HTMLElement
      expect(systemMain.style.getPropertyValue('--window-controls-inset-x')).toBe('')
    }
  )

  it.each(['windows', 'linux'] as const)(
    'renders no window controls with the system frame on %s',
    platform => {
      renderOn(platform, 'system')

      expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
      expect(screen.getByTestId('content')).toBeInTheDocument()
    }
  )

  it.each(['mac', 'browser'] as const)('renders no window controls on %s', platform => {
    renderOn(platform, 'custom')

    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })

  it('uses one flat layout with no icon rail or title bar row on every platform', () => {
    const structure = (platform: keyof typeof PLATFORMS) => {
      const view = renderOn(platform, 'system')
      const main = view.container.querySelector('main')
      const result = {
        mainClass: main?.className,
        hasAside: view.container.querySelector('aside') !== null,
        hasInset: view.container.querySelector('.rounded-xl') !== null,
      }
      view.unmount()
      return result
    }

    const windows = structure('windows')
    expect(windows.hasAside).toBe(false)
    expect(windows.hasInset).toBe(false)
    expect(structure('linux')).toEqual(windows)
    expect(structure('mac')).toEqual(windows)
  })

  it('moves the macOS traffic lights only on macOS', async () => {
    renderOn('windows', 'custom').unmount()
    renderOn('linux', 'custom').unmount()
    expect(trafficLight.set).not.toHaveBeenCalled()

    renderOn('mac')
    await waitFor(() => expect(trafficLight.set).toHaveBeenCalledWith(4, 8))
  })

  describe('macOS Library sidebar show/hide', () => {
    function ChromeProbe() {
      const chrome = useLibraryChrome()
      return (
        <button type="button" data-testid="probe" onClick={chrome.toggle}>
          {`hidden=${chrome.hidden} drawer=${chrome.drawer} open=${chrome.drawerOpen}`}
        </button>
      )
    }
    const renderMac = (width: number) => {
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: width })
      platformState.current = PLATFORMS.mac
      return render(
        <MemoryRouter>
          <LibraryMainLayout>
            <ChromeProbe />
          </LibraryMainLayout>
        </MemoryRouter>
      )
    }
    const probe = () => screen.getByTestId('probe').textContent

    it('hides the sidebar inline and remembers it in the standard tier', () => {
      localStorage.clear()
      const { unmount } = renderMac(1280)
      expect(probe()).toBe('hidden=false drawer=false open=false')

      fireEvent.click(screen.getByTestId('probe'))
      expect(probe()).toBe('hidden=true drawer=false open=false')
      unmount()

      renderMac(1280)
      expect(probe()).toBe('hidden=true drawer=false open=false')
      // ⌃⌘S drives the same toggle.
      act(() => shortcuts.handlers.get('nav.toggleSidebar')?.())
      expect(probe()).toBe('hidden=false drawer=false open=false')
      localStorage.clear()
    })

    it('hides the sidebar in the compact tier and opens it as a drawer', () => {
      localStorage.clear()
      renderMac(900)
      expect(probe()).toBe('hidden=true drawer=true open=false')

      fireEvent.click(screen.getByTestId('probe'))
      expect(probe()).toBe('hidden=true drawer=true open=true')

      // Widening past the tier shuts the drawer and shows the sidebar inline.
      act(() => {
        Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1280 })
        window.dispatchEvent(new Event('resize'))
      })
      expect(probe()).toBe('hidden=false drawer=false open=false')
      // The compact-tier toggle did not touch the remembered inline choice.
      expect(localStorage.getItem('uc.library.hidden.v1')).toBeNull()
    })
  })
})
