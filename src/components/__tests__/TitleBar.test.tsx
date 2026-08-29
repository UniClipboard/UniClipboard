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
  usePlatform: () => ({ isMac: false, isTauri: true }),
}))

vi.mock('@/hooks/useWindowFrame', () => ({
  useWindowFrame: () => ({ hasCustomWindowControls: true }),
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

  it('在 Linux 自绘窗口框中启用窗口控制按钮', async () => {
    render(<TitleBar />)

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
