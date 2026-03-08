import { describe, it, expect, beforeEach } from 'vitest'
import { applyThemePreset, getThemePreviewDots, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'

describe('theme-engine', () => {
  let root: HTMLElement

  beforeEach(() => {
    root = document.documentElement
    // Clear any previously applied inline CSS variables
    const style = root.style
    style.removeProperty('--background')
    style.removeProperty('--primary')
    style.removeProperty('--border')
  })

  it('applies a known preset in light mode and sets key CSS variables', () => {
    applyThemePreset('zinc', 'light', root)

    const background = root.style.getPropertyValue('--background')
    const primary = root.style.getPropertyValue('--primary')
    const border = root.style.getPropertyValue('--border')

    expect(background.trim()).not.toBe('')
    expect(primary.trim()).not.toBe('')
    expect(border.trim()).not.toBe('')
  })

  it('applies a known preset in dark mode with values differing from light where expected', () => {
    applyThemePreset('zinc', 'light', root)
    const lightBackground = root.style.getPropertyValue('--background')

    applyThemePreset('zinc', 'dark', root)
    const darkBackground = root.style.getPropertyValue('--background')

    expect(lightBackground.trim()).not.toBe('')
    expect(darkBackground.trim()).not.toBe('')
    expect(darkBackground.trim()).not.toBe(lightBackground.trim())
  })

  it('falls back to the default preset when an unknown theme name is provided', () => {
    applyThemePreset(DEFAULT_THEME_COLOR, 'light', root)
    const defaultBackground = root.style.getPropertyValue('--background')

    applyThemePreset('unknown-theme', 'light', root)
    const fallbackBackground = root.style.getPropertyValue('--background')
    const dataThemeAttr = root.getAttribute('data-theme')

    expect(fallbackBackground.trim()).toBe(defaultBackground.trim())
    expect(dataThemeAttr).toBe(DEFAULT_THEME_COLOR)
  })

  it('returns 3-4 preview dots for a theme', () => {
    const dots = getThemePreviewDots('zinc', 'light')

    expect(Array.isArray(dots)).toBe(true)
    expect(dots.length === 3 || dots.length === 4).toBe(true)
    for (const value of dots) {
      expect(typeof value).toBe('string')
      expect(value.trim()).not.toBe('')
    }
  })

  it('falls back to default preset preview dots for unknown theme names', () => {
    const defaultDots = getThemePreviewDots(DEFAULT_THEME_COLOR, 'light')
    const unknownDots = getThemePreviewDots('unknown-theme', 'light')

    expect(unknownDots).toEqual(defaultDots)
  })
})
