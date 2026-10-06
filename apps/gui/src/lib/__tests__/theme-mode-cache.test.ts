import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  readCachedThemeMode,
  THEME_MODE_STORAGE_KEY,
  writeCachedThemeMode,
} from '@/lib/theme-mode-cache'

const mockSystemDark = (dark: boolean) =>
  vi.stubGlobal(
    'matchMedia',
    vi
      .fn()
      .mockReturnValue({ matches: dark, addEventListener: vi.fn(), removeEventListener: vi.fn() })
  )

afterEach(() => {
  window.localStorage.clear()
  vi.unstubAllGlobals()
})

describe('theme mode cache', () => {
  it('prefers the cached mode over the system preference', () => {
    mockSystemDark(true)
    writeCachedThemeMode('light')
    expect(readCachedThemeMode()).toBe('light')
  })

  it('falls back to the system preference without a cached mode', () => {
    mockSystemDark(true)
    expect(readCachedThemeMode()).toBe('dark')
    mockSystemDark(false)
    expect(readCachedThemeMode()).toBe('light')
  })

  it('ignores an invalid cached value', () => {
    mockSystemDark(false)
    window.localStorage.setItem(THEME_MODE_STORAGE_KEY, 'sepia')
    expect(readCachedThemeMode()).toBe('light')
  })

  it('survives unavailable storage', () => {
    mockSystemDark(true)
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(() => writeCachedThemeMode('dark')).not.toThrow()
    expect(readCachedThemeMode()).toBe('dark')
  })
})
