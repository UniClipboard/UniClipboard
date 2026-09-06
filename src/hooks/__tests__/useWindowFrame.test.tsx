import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { WindowShell } from '@/layouts/WindowShell'
import { applyWindowFrameDocumentState, resolveWindowFrameMode } from '@/lib/window-frame'

const mocks = vi.hoisted(() => ({
  platform: { isWindows: false, isMac: false, isLinux: true, isTauri: true },
  setDecorations: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/hooks/usePlatform', () => ({ usePlatform: () => mocks.platform }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setDecorations: mocks.setDecorations }),
}))

function FrameControls() {
  const { setUseSystemWindowFrame } = useWindowFrame()
  return (
    <>
      <button type="button" onClick={() => setUseSystemWindowFrame(true)}>
        System
      </button>
      <button type="button" onClick={() => setUseSystemWindowFrame(false)}>
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
    delete document.documentElement.dataset.ucCustomWindowFrame
  })

  it.each(['linux', 'windows'] as const)(
    'keeps %s corner and background policy after switching',
    async platform => {
      mocks.platform.isLinux = platform === 'linux'
      mocks.platform.isWindows = platform === 'windows'
      const rounded = platform === 'windows'
      applyWindowFrameDocumentState(resolveWindowFrameMode(mocks.platform, false).hasRoundedWindow)
      const { container } = render(<WindowShell titleBar={<FrameControls />}>Content</WindowShell>)
      const shell = container.firstElementChild

      await act(async () => screen.getByText('System').click())
      expect(mocks.setDecorations).toHaveBeenLastCalledWith(true)
      expect(shell).not.toHaveClass('rounded-xl')
      expect(document.documentElement.dataset.ucCustomWindowFrame).toBe('false')

      await act(async () => screen.getByText('Custom').click())
      expect(mocks.setDecorations).toHaveBeenLastCalledWith(false)
      expect(shell?.classList.contains('rounded-xl')).toBe(rounded)
      expect(document.documentElement.dataset.ucCustomWindowFrame).toBe(String(rounded))
    }
  )
})
