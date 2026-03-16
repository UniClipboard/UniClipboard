import { listen } from '@tauri-apps/api/event'
import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useThemeSync } from '../useThemeSync'
import { invokeWithTrace } from '@/lib/tauri-command'
import { applyThemePreset } from '@/lib/theme-engine'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

vi.mock('@/lib/theme-engine', async importOriginal => {
  const actual = await importOriginal<typeof import('@/lib/theme-engine')>()
  return {
    ...actual,
    applyThemePreset: vi.fn(),
  }
})

const mockListen = vi.mocked(listen)
const mockInvokeWithTrace = vi.mocked(invokeWithTrace)
const mockApplyThemePreset = vi.mocked(applyThemePreset)

describe('useThemeSync', () => {
  let settingChangedCallback: ((event: { payload: { settingJson: string } }) => void) | null = null
  let mediaQueryListener: ((event: MediaQueryListEvent) => void) | null = null
  const unlisten = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
    settingChangedCallback = null
    mediaQueryListener = null

    mockInvokeWithTrace.mockResolvedValue({
      general: { theme: 'dark', theme_color: 'blue' },
    })

    mockListen.mockImplementation(async (_channel: string, callback: unknown) => {
      settingChangedCallback = callback as (event: { payload: { settingJson: string } }) => void
      return unlisten
    })

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        matches: false,
        addEventListener: vi.fn((_event: string, handler: (event: MediaQueryListEvent) => void) => {
          mediaQueryListener = handler
        }),
        removeEventListener: vi.fn(),
      })),
    })
  })

  it('loads settings and applies theme on mount', async () => {
    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(mockInvokeWithTrace).toHaveBeenCalledWith('get_settings')
      expect(mockApplyThemePreset).toHaveBeenCalledWith('blue', 'dark', document.documentElement)
      expect(document.documentElement.classList.contains('dark')).toBe(true)
    })
  })

  it('reapplies theme when setting-changed fires', async () => {
    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(settingChangedCallback).not.toBeNull()
    })

    settingChangedCallback?.({
      payload: {
        settingJson: JSON.stringify({ general: { theme: 'light', theme_color: 'rose' } }),
      },
    })

    expect(mockApplyThemePreset).toHaveBeenLastCalledWith('rose', 'light', document.documentElement)
    expect(document.documentElement.classList.contains('light')).toBe(true)
  })

  it('reacts to system theme changes when theme follows system', async () => {
    mockInvokeWithTrace.mockResolvedValue({
      general: { theme: 'system', theme_color: 'green' },
    })
    const matchMediaMock = vi.fn().mockImplementation(() => ({
      matches: true,
      addEventListener: vi.fn((_event: string, handler: (event: MediaQueryListEvent) => void) => {
        mediaQueryListener = handler
      }),
      removeEventListener: vi.fn(),
    }))
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: matchMediaMock,
    })

    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(mediaQueryListener).not.toBeNull()
    })

    mediaQueryListener?.({ matches: false } as MediaQueryListEvent)

    expect(mockApplyThemePreset).toHaveBeenLastCalledWith('green', 'dark', document.documentElement)
  })
})
