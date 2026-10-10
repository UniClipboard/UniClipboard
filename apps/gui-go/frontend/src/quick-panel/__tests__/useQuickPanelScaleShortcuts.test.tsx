import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { useQuickPanelScaleShortcuts } from '../hooks/useQuickPanelScaleShortcuts'

const { adjust, config } = vi.hoisted(() => ({
  adjust: vi.fn().mockReturnValue({ textPercent: 100, windowPercent: 100 }),
  config: { overrides: {} as Record<string, string> },
}))
vi.mock('../window-layout', () => ({ adjustQuickPanelScale: adjust }))
vi.mock('../hooks/useQuickPanelShortcutOverrides', () => ({
  useQuickPanelShortcutOverrides: () => config.overrides,
}))
function Controls() {
  useQuickPanelScaleShortcuts(() => {})
  return <input aria-label="search" />
}
function press(key: string, code: string, shiftKey = false) {
  fireEvent.keyDown(screen.getByRole('textbox'), { key, code, ctrlKey: true, shiftKey })
  fireEvent.keyUp(screen.getByRole('textbox'), { key, code, ctrlKey: true, shiftKey })
}
beforeEach(() => {
  adjust.mockClear()
  config.overrides = {}
})
it('routes all four default keys from the search input', () => {
  render(
    <ShortcutProvider>
      <Controls />
    </ShortcutProvider>
  )
  press('=', 'Equal')
  press('-', 'Minus')
  press('+', 'Equal', true)
  press('_', 'Minus', true)
  expect(adjust.mock.calls.map(call => call[0])).toEqual([
    'windowIncrease',
    'windowDecrease',
    'textIncrease',
    'textDecrease',
  ])
})
it('uses custom keys immediately and removes replaced or disabled defaults', () => {
  config.overrides = { 'quickPanel.windowIncrease': 'ctrl+k', 'quickPanel.textIncrease': '' }
  const { rerender } = render(
    <ShortcutProvider>
      <Controls />
    </ShortcutProvider>
  )
  press('=', 'Equal')
  press('+', 'Equal', true)
  expect(adjust).not.toHaveBeenCalled()
  press('k', 'KeyK')
  expect(adjust).toHaveBeenCalledExactlyOnceWith('windowIncrease')
  adjust.mockClear()
  config.overrides = { 'quickPanel.windowIncrease': 'ctrl+j' }
  rerender(
    <ShortcutProvider>
      <Controls />
    </ShortcutProvider>
  )
  press('k', 'KeyK')
  expect(adjust).not.toHaveBeenCalled()
  press('j', 'KeyJ')
  expect(adjust).toHaveBeenCalledExactlyOnceWith('windowIncrease')
})
it('supports recorded two-step chords', () => {
  config.overrides = { 'quickPanel.windowIncrease': 'ctrl+k ctrl+j' }
  render(
    <ShortcutProvider>
      <Controls />
    </ShortcutProvider>
  )
  press('k', 'KeyK')
  expect(adjust).not.toHaveBeenCalled()
  press('j', 'KeyJ')
  expect(adjust).toHaveBeenCalledExactlyOnceWith('windowIncrease')
})
