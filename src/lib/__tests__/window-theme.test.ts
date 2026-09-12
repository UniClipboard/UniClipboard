import { beforeEach, describe, expect, it, vi } from 'vitest'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import type { DesktopTheme } from '@/lib/desktop-theme'
import { startThemeTransition } from '@/lib/theme-transition'
import { createWindowThemeController, initializeWindowTheme } from '@/lib/window-theme'
import { makeBaseSettings } from '@/test/fixtures/settings'

vi.mock('@/lib/desktop-theme', () => ({ subscribeDesktopTheme: vi.fn() }))
vi.mock('@/lib/theme-transition', () => ({
  startThemeTransition: vi.fn((apply: () => void) => apply()),
}))

let receive: Parameters<typeof subscribeDesktopTheme>[0]
const unsubscribe = vi.fn()
const palette = (background: string, dark = true): DesktopTheme => ({
  dark,
  variables: { '--background': background, '--primary': '#7fbbb3', '--card': '#343f44' },
})
const general = (theme: 'light' | 'dark' | 'system') =>
  makeBaseSettings({ general: { theme } }).general

beforeEach(() => {
  vi.clearAllMocks()
  document.documentElement.removeAttribute('style')
  vi.mocked(subscribeDesktopTheme).mockImplementation(callback => {
    receive = callback
    return unsubscribe
  })
})

describe.each([
  ['main app', true],
  ['quick panel', false],
] as const)('%s theme owner', (_name, animate) => {
  it('follows same-mode and light/dark desktop changes', () => {
    const controller = createWindowThemeController(animate)
    controller.setGeneral(general('system'))
    for (const next of [palette('#2d353b'), palette('#1e1e2e'), palette('#ffffff', false)]) {
      receive(next)
      expect(document.documentElement.style.getPropertyValue('--background')).toBe(
        next.variables['--background']
      )
      expect(document.documentElement.classList.contains(next.dark ? 'dark' : 'light')).toBe(true)
      expect(document.documentElement.dataset.theme).toBe('desktop')
    }
    controller.dispose()
    expect(unsubscribe).toHaveBeenCalledOnce()
  })

  it('overrides manual mode while enabled and restores saved settings when disabled', () => {
    const controller = createWindowThemeController(animate)
    controller.setGeneral(general('light'))
    const original = document.documentElement.style.getPropertyValue('--background')
    receive(palette('#2d353b'))
    expect(document.documentElement.style.getPropertyValue('--background')).toBe('#2d353b')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    receive(null)
    expect(document.documentElement.style.getPropertyValue('--background')).toBe(original)
    expect(document.documentElement.classList.contains('light')).toBe(true)
    controller.dispose()
  })

  it('applies a palette received before settings and stops updating after cleanup', () => {
    const controller = createWindowThemeController(animate)
    receive(palette('#2d353b'))
    expect(document.documentElement.style.getPropertyValue('--background')).toBe('')
    controller.setGeneral(general('system'))
    expect(document.documentElement.style.getPropertyValue('--background')).toBe('#2d353b')
    controller.dispose()
    receive(palette('#ffffff', false))
    expect(document.documentElement.style.getPropertyValue('--background')).toBe('#2d353b')
  })
})

it('prevents an older animated update from overwriting a newer desktop change', () => {
  let pending!: () => void
  vi.mocked(startThemeTransition).mockImplementationOnce(apply => {
    pending = apply
  })
  const controller = createWindowThemeController(true)
  controller.setGeneral(general('dark'))
  controller.setGeneral(general('system'))
  receive(palette('#2d353b'))
  pending()
  expect(document.documentElement.style.getPropertyValue('--background')).toBe('#2d353b')
  controller.dispose()
})

it('updates compositor rounding even when the manual palette is unchanged', () => {
  const controller = createWindowThemeController()
  controller.setGeneral(general('light'))
  receive(null, 12)
  expect(document.documentElement.style.getPropertyValue('--desktop-window-radius')).toBe('12px')
  receive(null, 0)
  expect(document.documentElement.style.getPropertyValue('--desktop-window-radius')).toBe('0px')
  controller.dispose()
})

it('waits for and applies the desktop palette before startup is ready', async () => {
  const ready = vi.fn()
  const initialization = initializeWindowTheme().then(ready)
  await Promise.resolve()
  expect(ready).not.toHaveBeenCalled()
  expect(document.documentElement.style.getPropertyValue('--background')).toBe('')
  receive(palette('#2d353b'))
  await initialization
  expect(document.documentElement.style.getPropertyValue('--background')).toBe('#2d353b')
  expect(unsubscribe).toHaveBeenCalledOnce()
})
