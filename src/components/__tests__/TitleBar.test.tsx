import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { TitleBar } from '@/components/TitleBar'

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
  usePlatform: () => ({
    isWindows: false,
    isMac: false,
    isLinux: true,
    isTauri: true,
    hasCustomWindowControls: true,
    reduceVisualEffects: true,
  }),
}))

vi.mock('@/lib/ipc', () => ({
  commands: {
    setTrafficLightPosition: vi.fn().mockResolvedValue(undefined),
  },
}))

vi.mock('@/components/DevProfileIndicator', () => ({
  DevProfileIndicator: () => null,
}))

describe('TitleBar', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    windowMocks.isMaximized.mockResolvedValue(false)
  })

  it('在 Linux 中显示并启用窗口控制按钮', async () => {
    render(<TitleBar />)

    expect(screen.getByRole('button', { name: '关闭' }).parentElement).toHaveClass('mr-2')

    fireEvent.click(screen.getByRole('button', { name: '最小化' }))
    fireEvent.click(screen.getByRole('button', { name: '最大化' }))
    fireEvent.click(screen.getByRole('button', { name: '关闭' }))

    await waitFor(() => {
      expect(windowMocks.minimize).toHaveBeenCalledOnce()
      expect(windowMocks.maximize).toHaveBeenCalledOnce()
      expect(windowMocks.close).toHaveBeenCalledOnce()
    })
  })
})
