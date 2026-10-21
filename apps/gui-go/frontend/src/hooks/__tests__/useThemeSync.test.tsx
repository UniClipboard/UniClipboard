import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings } from '@/api/daemon'
import { resetSettingsSyncForTest } from '@/lib/settings-sync'
import { applyThemePreset } from '@/lib/theme-engine'
import { emitDaemonWsEvent } from '@/test/fixtures/settings-ws'
import { useThemeSync } from '../useThemeSync'

vi.mock('@/lib/daemon-ws', () => import('@/test/fixtures/settings-ws'))
vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
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

const mockGetSettings = vi.mocked(getSettings)
const mockApplyThemePreset = vi.mocked(applyThemePreset)

describe('useThemeSync', () => {
  let mediaQueryListener: ((event: MediaQueryListEvent) => void) | null = null
  let prefersDark = false

  beforeEach(() => {
    vi.clearAllMocks()
    resetSettingsSyncForTest()
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

  it('loads settings after the settings snapshot', async () => {
    renderHook(() => useThemeSync())

    await waitFor(() => {
      expect(mockGetSettings).toHaveBeenCalled()
      expect(mockApplyThemePreset).toHaveBeenCalledWith('blue', 'dark', document.documentElement)
      expect(document.documentElement.classList.contains('dark')).toBe(true)
    })
  })

  it('reapplies theme when the daemon reports a settings change', async () => {
    renderHook(() => useThemeSync())
    await waitFor(() => expect(mockGetSettings).toHaveBeenCalledTimes(1))

    mockGetSettings.mockResolvedValue({
      general: {
        theme: 'light',
        themeColor: null,
        themeColorLight: 'rose',
        themeColorDark: 'claude',
      },
    } as never)
    act(() =>
      emitDaemonWsEvent('settings.changed', {
        owner: 'desktop',
        sections: ['general'],
        revision: 1,
      })
    )

    await waitFor(() => {
      expect(mockApplyThemePreset).toHaveBeenLastCalledWith(
        'rose',
        'light',
        document.documentElement
      )
      expect(document.documentElement.classList.contains('light')).toBe(true)
    })
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

  it('does not let a pending settings query overwrite a newer read', async () => {
    let resolveSettings!: (settings: Awaited<ReturnType<typeof getSettings>>) => void
    mockGetSettings
      .mockReturnValueOnce(
        new Promise(resolve => {
          resolveSettings = resolve
        })
      )
      .mockResolvedValue({ general: { theme: 'light', themeColorLight: 'rose' } } as never)
    renderHook(() => useThemeSync())
    await waitFor(() => expect(mockGetSettings).toHaveBeenCalledTimes(1))
    act(() =>
      emitDaemonWsEvent('settings.changed', {
        owner: 'desktop',
        sections: ['general'],
        revision: 1,
      })
    )
    await act(async () => {
      resolveSettings({ general: { theme: 'dark', themeColorDark: 'blue' } } as never)
    })
    await waitFor(() =>
      expect(mockApplyThemePreset).toHaveBeenLastCalledWith(
        'rose',
        'light',
        document.documentElement
      )
    )
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
