import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import MainLayout from '../MainLayout'

const platformState = vi.hoisted(() => ({
  current: {
    isWindows: false,
    isMac: false,
    isLinux: false,
    isTauri: false,
  },
}))

const windowFrameState = vi.hoisted(() => ({
  useSystemWindowFrame: false,
}))

const windowMocks = vi.hoisted(() => ({
  close: vi.fn().mockResolvedValue(undefined),
  isMaximized: vi.fn().mockResolvedValue(false),
  maximize: vi.fn().mockResolvedValue(undefined),
  minimize: vi.fn().mockResolvedValue(undefined),
  onResized: vi.fn().mockResolvedValue(() => {}),
  unmaximize: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => windowMocks,
}))

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: () => platformState.current,
}))

vi.mock('@/hooks/useWindowFrame', () => ({
  useWindowFrame: () => ({
    ...windowFrameState,
    hasCustomWindowControls:
      platformState.current.isTauri &&
      !platformState.current.isMac &&
      !windowFrameState.useSystemWindowFrame,
  }),
}))

const shortcuts = vi.hoisted(() => ({ handlers: new Map<string, () => void>() }))
vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: ({ id, handler }: { id?: string; handler: () => void }) => {
    if (id) shortcuts.handlers.set(id, handler)
  },
}))
vi.mock('@/lib/ipc', () => ({
  commands: { setTrafficLightPosition: vi.fn().mockResolvedValue(undefined) },
}))

vi.mock('@/contexts/titlebar-slot-context', () => ({
  useTitleBarSlot: () => ({ rightSlotHost: null }),
}))

vi.mock('@/components', () => ({
  Sidebar: ({ className }: { className?: string }) => (
    <aside data-testid="sidebar" className={className} />
  ),
}))

const renderLayout = () =>
  render(
    <MemoryRouter>
      <MainLayout>
        <div data-testid="content" />
      </MainLayout>
    </MemoryRouter>
  )

describe('MainLayout', () => {
  it('Windows 历史和设备共用布局显示并启用三个窗口按钮', async () => {
    platformState.current = {
      isWindows: true,
      isMac: false,
      isLinux: false,
      isTauri: true,
    }
    windowFrameState.useSystemWindowFrame = false

    renderLayout()

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
  })

  it('Linux 自绘窗口框使用与标题栏一致的内嵌布局', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
    }
    windowFrameState.useSystemWindowFrame = false

    const { container } = renderLayout()
    const main = container.querySelector('main')
    const inset = main?.querySelector('.pb-2.pr-2')

    expect(inset).toBeInTheDocument()
    expect(inset?.firstElementChild).toHaveClass('rounded-xl')
    expect(screen.getByRole('button', { name: '关闭' })).toBeInTheDocument()
  })

  it('Linux 系统窗口框使用平面布局', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
    }
    windowFrameState.useSystemWindowFrame = true

    const { container } = renderLayout()
    const main = container.querySelector('main')

    expect(main).toHaveClass('bg-card')
    expect(main?.querySelector('.pb-2.pr-2')).not.toBeInTheDocument()
    expect(main?.querySelector('.rounded-xl')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '关闭' })).not.toBeInTheDocument()
  })

  it('侧栏固定为窄栏并提供历史与设备两个页面入口', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: false,
      isTauri: false,
    }
    windowFrameState.useSystemWindowFrame = false

    const { container } = renderLayout()
    const sidebar = container.querySelector('aside')

    expect(sidebar).toHaveClass('w-12')
    expect(screen.getByRole('link', { name: 'History' })).toHaveAttribute('href', '/history')
    expect(screen.getByRole('link', { name: 'Devices' })).toHaveAttribute('href', '/devices')
    expect(screen.queryByRole('button', { name: /sidebar/i })).not.toBeInTheDocument()
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
      platformState.current = { isWindows: false, isMac: true, isLinux: false, isTauri: true }
      return render(
        <MemoryRouter>
          <MainLayout>
            <ChromeProbe />
          </MainLayout>
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
