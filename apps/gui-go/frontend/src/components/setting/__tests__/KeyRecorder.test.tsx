import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { KeyRecorder } from '@/components/setting/KeyRecorder'

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
vi.mock('@/hooks/useShortcutLayer', () => ({ useShortcutLayer: vi.fn() }))

afterEach(cleanup)

function renderRecorder() {
  const onConfirm = vi.fn()
  render(
    <KeyRecorder
      shortcutId="global.toggleQuickPanel"
      scope="global"
      currentOverrides={{}}
      onConfirm={onConfirm}
      onCancel={vi.fn()}
    />
  )
  const field = screen.getByRole('group')
  field.focus()
  return { field, onConfirm }
}

function save() {
  fireEvent.click(screen.getByRole('button', { name: 'settings.sections.shortcuts.save' }))
}

describe('KeyRecorder keyboard layout handling', () => {
  it.each([
    ['Option symbol', '√', 'KeyV', 'v'],
    ['dead accent', 'Dead', 'KeyE', 'e'],
    ['non-Latin letter', 'м', 'KeyV', 'v'],
    ['Option digit symbol', '¡', 'Digit1', '1'],
    ['digit plus symbol', '+', 'Digit1', '1'],
    ['digit underscore symbol', '_', 'Digit8', '8'],
    ['AZERTY letter', 'a', 'KeyQ', 'a'],
    ['Dvorak letter', 'v', 'Period', 'v'],
    ['uppercase Latin letter', 'V', 'KeyV', 'v'],
    ['ASCII digit', '2', 'Digit2', '2'],
    ['equal key', '+', 'Equal', 'equal'],
    ['minus key', '_', 'Minus', 'minus'],
    ['numpad add', '+', 'NumpadAdd', 'add'],
    ['numpad subtract', '-', 'NumpadSubtract', 'subtract'],
    ['arrow key', 'ArrowUp', 'ArrowUp', 'arrowup'],
    ['function key', 'F5', 'F5', 'f5'],
  ])('records %s as a bindable key', (_name, key, code, expected) => {
    const { field, onConfirm } = renderRecorder()
    fireEvent.keyDown(field, { key, code, altKey: true, metaKey: true })
    save()
    expect(onConfirm).toHaveBeenCalledWith(`alt+meta+${expected}`, undefined)
  })

  it('ignores bare modifiers and repeats while recording both chord segments', () => {
    const { field, onConfirm } = renderRecorder()
    fireEvent.keyDown(field, { key: 'Alt', code: 'AltLeft', altKey: true })
    fireEvent.keyDown(field, { key: '√', code: 'KeyV', altKey: true, metaKey: true })
    fireEvent.keyDown(field, {
      key: '√',
      code: 'KeyV',
      altKey: true,
      metaKey: true,
      repeat: true,
    })
    fireEvent.keyDown(field, { key: 'Dead', code: 'KeyE', altKey: true, metaKey: true })
    save()
    expect(onConfirm).toHaveBeenCalledWith('alt+meta+v alt+meta+e', undefined)
  })
})
