import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { WindowControls } from '@/components/WindowControls'

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
  usePlatform: () => ({ isWindows: true, isMac: false, isLinux: false, isTauri: true }),
}))

describe('WindowControls', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    windowMocks.isMaximized.mockResolvedValue(false)
  })

  it('minimizes, maximizes and closes the window', async () => {
    render(<WindowControls />)

    fireEvent.click(screen.getByRole('button', { name: '最小化' }))
    fireEvent.click(screen.getByRole('button', { name: '最大化' }))
    fireEvent.click(screen.getByRole('button', { name: '关闭' }))

    await waitFor(() => {
      expect(windowMocks.minimize).toHaveBeenCalledOnce()
      expect(windowMocks.maximize).toHaveBeenCalledOnce()
      expect(windowMocks.close).toHaveBeenCalledOnce()
    })
  })

  it('offers restore while the window is maximized', async () => {
    windowMocks.isMaximized.mockResolvedValue(true)
    render(<WindowControls />)

    fireEvent.click(await screen.findByRole('button', { name: '还原' }))

    await waitFor(() => expect(windowMocks.unmaximize).toHaveBeenCalledOnce())
  })

  it('keeps its buttons out of the window drag region', () => {
    render(<WindowControls />)

    expect(screen.getByRole('button', { name: '关闭' })).toHaveAttribute(
      'data-tauri-drag-region',
      'false'
    )
  })
})
