import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  DEFAULT_UI_SCALE,
  UI_SCALE_STORAGE_KEY,
  adjustUiScale,
  initializeUiScale,
  readStoredUiScale,
  setUiScale,
} from '@/lib/ui-scale'

describe('ui scale', () => {
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('zoom')
  })

  afterEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('zoom')
  })

  it('applies the stored scale on startup', () => {
    localStorage.setItem(UI_SCALE_STORAGE_KEY, '1.2')

    const cleanup = initializeUiScale()

    expect(readStoredUiScale()).toBe(1.2)
    expect(document.documentElement.style.getPropertyValue('--app-ui-scale')).toBe('1.2')

    cleanup()
  })

  it('adjusts and persists the scale within bounds', () => {
    expect(setUiScale(DEFAULT_UI_SCALE)).toBe(DEFAULT_UI_SCALE)

    expect(adjustUiScale('in')).toBe(1.1)
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe('1.1')
    expect(document.documentElement.style.getPropertyValue('--app-ui-scale')).toBe('1.1')

    expect(adjustUiScale('out')).toBe(DEFAULT_UI_SCALE)
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe('1')

    expect(setUiScale(9)).toBe(1.5)
    expect(setUiScale(0.1)).toBe(0.8)
  })
})
