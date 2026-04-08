import { render } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { GlobalShortcuts } from '@/components/GlobalShortcuts'
import { useShortcut } from '@/hooks/useShortcut'
import { UI_SCALE_STORAGE_KEY } from '@/lib/ui-scale'

vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: vi.fn(),
}))

describe('GlobalShortcuts zoom integration', () => {
  beforeEach(() => {
    vi.mocked(useShortcut).mockReset()
    localStorage.clear()
    document.documentElement.style.removeProperty('zoom')
  })

  afterEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('zoom')
  })

  it('registers zoom shortcuts and their handlers update the stored scale', () => {
    render(
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route path="*" element={<GlobalShortcuts />} />
        </Routes>
      </MemoryRouter>
    )

    const shortcutConfigs = vi.mocked(useShortcut).mock.calls.map(([config]) => config)
    const zoomIn = shortcutConfigs.find(config => config.id === 'global.zoomIn')
    const zoomOut = shortcutConfigs.find(config => config.id === 'global.zoomOut')

    expect(zoomIn).toBeDefined()
    expect(zoomOut).toBeDefined()

    zoomIn?.handler()
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe('1.1')
    expect(document.documentElement.style.getPropertyValue('--app-ui-scale')).toBe('1.1')

    zoomOut?.handler()
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe('1')
  })
})
