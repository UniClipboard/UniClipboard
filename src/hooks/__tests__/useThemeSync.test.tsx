import { listen } from '@tauri-apps/api/event'
import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useThemeSync } from '../useThemeSync'
import { getSettings } from '@/api/daemon'
import { applyThemePreset } from '@/lib/theme-engine'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

vi.mock('@/api/daemon', () => ({
  getSettings: vi.fn(),
}))

vi.mock('@/lib/theme-engine', async importOriginal => {
  const actual = await importOriginal<typeof import('@/lib/theme-engine')>()
  return {
    ...actual,
    applyThemePreset: vi.fn(),
  }
})

const mockListen = vi.mocked(listen)
const mockGetSettings = vi.mocked(getSettings)
const mockApplyThemePreset = vi.mocked(applyThemePreset)

describe('useThemeSync', () => {
  let settingsChangedCallback: ((event: { payload: { settingJson: string } }) => void) | null = null
  let mediaQueryListener: ((event: MediaQueryListEvent) => void) | null = null
  let prefersDark = false
  const unlisten = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
    settingsChangedCallback = null
    mediaQueryListener = null
    prefersDark = false

    mockGetSettings.mockResolvedValue({
      general: {
        theme: 'dark',
        themeColor: null,
        themeColorLight: 'zinc',
        themeColorDark: 'blue',
      },
    } as never)

    mockListen.mockImplementation(async (eventName: string, callback: unknown) => {
      if (eventName === 'settings://changed') {
        settingsChangedCallback = callback as (event: { payload: { settingJson: string } }) => void
      }
      return unlisten
    })

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        get matches() {
          return prefersDark
        },
        addEventListener: vi.fn((_event: string, handler: (event: MediaQueryListEvent) => void) => {
          mediaQueryListener = handler
        }),
        removeEventListener: vi.fn(),
      })),
    })
  })

  it('loads settings and subscribes to cross-window theme changes', async () => {
    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(mockGetSettings).toHaveBeenCalled()
      expect(mockListen).toHaveBeenCalledWith('settings://changed', expect.any(Function))
      expect(mockApplyThemePreset).toHaveBeenCalledWith('blue', 'dark', document.documentElement)
      expect(document.documentElement.classList.contains('dark')).toBe(true)
    })
  })

  it('reapplies theme when another window broadcasts settings changes', async () => {
    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(settingsChangedCallback).not.toBeNull()
    })

    settingsChangedCallback?.({
      payload: {
        settingJson: JSON.stringify({
          general: {
            theme: 'light',
            themeColor: null,
            themeColorLight: 'rose',
            themeColorDark: 'claude',
          },
        }),
      },
    })

    expect(mockApplyThemePreset).toHaveBeenLastCalledWith('rose', 'light', document.documentElement)
    expect(document.documentElement.classList.contains('light')).toBe(true)
  })

  it('reacts to system theme changes when current setting follows the system', async () => {
    prefersDark = true
    mockGetSettings.mockResolvedValue({
      general: {
        theme: 'system',
        themeColor: null,
        themeColorLight: 'zinc',
        themeColorDark: 'green',
      },
    } as never)

    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(mediaQueryListener).not.toBeNull()
      expect(mockApplyThemePreset).toHaveBeenLastCalledWith(
        'green',
        'dark',
        document.documentElement
      )
    })

    prefersDark = false
    mediaQueryListener?.({ matches: false } as MediaQueryListEvent)

    // 切到 light 时改用 themeColorLight = "zinc"
    expect(mockApplyThemePreset).toHaveBeenLastCalledWith('zinc', 'light', document.documentElement)
  })

  it('falls back to legacy themeColor when split fields are null', async () => {
    mockGetSettings.mockResolvedValue({
      general: {
        theme: 'light',
        themeColor: 'catppuccin',
        themeColorLight: null,
        themeColorDark: null,
      },
    } as never)

    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(mockApplyThemePreset).toHaveBeenLastCalledWith(
        'catppuccin',
        'light',
        document.documentElement
      )
    })
  })
})
