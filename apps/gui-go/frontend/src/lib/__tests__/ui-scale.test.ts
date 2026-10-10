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
  const originalLocalStorageDescriptor = Object.getOwnPropertyDescriptor(window, 'localStorage')

  beforeEach(() => {
    localStorage.clear()
  })

  afterEach(() => {
    if (originalLocalStorageDescriptor) {
      Object.defineProperty(window, 'localStorage', originalLocalStorageDescriptor)
    }
    window.localStorage.clear()
  })

  it('applies the stored scale on startup', () => {
    localStorage.setItem(UI_SCALE_STORAGE_KEY, '1.2')

    const cleanup = initializeUiScale()

    expect(readStoredUiScale()).toBe(1.2)

    cleanup()
  })

  it('adjusts and persists the scale within bounds', () => {
    expect(setUiScale(DEFAULT_UI_SCALE)).toBe(DEFAULT_UI_SCALE)

    expect(adjustUiScale('in')).toBe(1.1)
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe('1.1')

    expect(adjustUiScale('out')).toBe(DEFAULT_UI_SCALE)
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe('1')

    expect(setUiScale(9)).toBe(1.5)
    expect(setUiScale(0.1)).toBe(0.8)
  })

  it('falls back to the default scale when storage reads throw', () => {
    const throwingStorage = {
      getItem: () => {
        throw new Error('denied')
      },
      setItem: () => undefined,
      removeItem: () => undefined,
      clear: () => undefined,
      key: () => null,
      length: 0,
    } as Storage

    expect(readStoredUiScale(throwingStorage)).toBe(DEFAULT_UI_SCALE)
    const cleanup = initializeUiScale(throwingStorage)
    cleanup()
  })

  it('ignores storage write errors when setting the scale', () => {
    const throwingStorage = {
      getItem: () => null,
      setItem: () => {
        throw new Error('quota')
      },
      removeItem: () => undefined,
      clear: () => undefined,
      key: () => null,
      length: 0,
    } as Storage

    expect(() => setUiScale(1.1, throwingStorage)).not.toThrow()
  })

  it('continues initialization when window.localStorage getter throws', () => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      get() {
        throw new Error('opaque origin')
      },
    })

    expect(readStoredUiScale()).toBe(DEFAULT_UI_SCALE)
    const cleanup = initializeUiScale()
    cleanup()
  })
})
