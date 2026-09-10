import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { WindowShell } from '@/layouts/WindowShell'

const mocks = vi.hoisted(() => ({
  platform: { isWindows: false, isMac: false, isLinux: true, isTauri: true },
  setDecorations: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/hooks/usePlatform', () => ({ usePlatform: () => mocks.platform }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setDecorations: mocks.setDecorations }),
}))

function FrameControls() {
  const { setWindowFramePreference } = useWindowFrame()
  return (
    <>
      <button type="button" onClick={() => setWindowFramePreference('system')}>
        System
      </button>
      <button type="button" onClick={() => setWindowFramePreference('custom')}>
        Custom
      </button>
    </>
  )
}

describe('window frame switching', () => {
  beforeEach(() => {
    localStorage.clear()
    mocks.setDecorations.mockClear()
  })
  afterEach(() => {
    cleanup()
  })

  it.each(['linux', 'windows'] as const)(
    'keeps %s corner and background policy after switching',
    async platform => {
      mocks.platform.isLinux = platform === 'linux'
      mocks.platform.isWindows = platform === 'windows'
      const { container } = render(<WindowShell titleBar={<FrameControls />}>Content</WindowShell>)
      const shell = container.firstElementChild

      await act(async () => screen.getByText('System').click())
      expect(mocks.setDecorations).toHaveBeenLastCalledWith(true)
      expect(shell).not.toHaveClass('rounded-xl')

      await act(async () => screen.getByText('Custom').click())
      expect(mocks.setDecorations).toHaveBeenLastCalledWith(false)
      expect(shell).not.toHaveClass('rounded-xl')
    }
  )
})
