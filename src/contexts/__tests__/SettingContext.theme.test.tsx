import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { DEFAULT_THEME_COLOR } from '@/constants/theme'
import { SettingProvider } from '@/contexts/SettingContext'
import { useSetting } from '@/hooks/useSetting'

vi.mock('@/hooks/useSetting', () => {
  return {
    useSetting: () => ({
      setting: {
        general: {
          theme: 'light',
          theme_color: DEFAULT_THEME_COLOR,
          language: 'en',
        },
      },
    }),
  }
})

describe('SettingProvider theme integration', () => {
  const matchMediaMock = {
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  } as unknown as MediaQueryList

  beforeEach(() => {
    // jsdom does not provide matchMedia by default; provide a minimal stub.
    ;(window as unknown as { matchMedia: (query: string) => MediaQueryList }).matchMedia = vi
      .fn()
      .mockReturnValue(matchMediaMock)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    document.documentElement.className = ''
    document.documentElement.removeAttribute('data-theme')
  })

  it('applies persisted theme_color on mount', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <SettingProvider>{children}</SettingProvider>
    )

    const { result } = renderHook(() => useSetting(), { wrapper })

    expect(result.current.setting?.general.theme_color).toBe(DEFAULT_THEME_COLOR)
    expect(document.documentElement.getAttribute('data-theme')).toBe(DEFAULT_THEME_COLOR)
  })

  it('falls back to default preset when theme_color is null', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <SettingProvider>{children}</SettingProvider>
    )

    const { result } = renderHook(() => useSetting(), { wrapper })

    act(() => {
      if (result.current.setting?.general) {
        result.current.setting.general.theme_color = null
      }
    })

    expect(document.documentElement.getAttribute('data-theme')).toBe(DEFAULT_THEME_COLOR)
  })

  it('system mode change listener still updates mode class', () => {
    const darkMatchMediaMock = {
      ...matchMediaMock,
      matches: true,
    } as unknown as MediaQueryList

    vi.spyOn(window, 'matchMedia').mockReturnValue(darkMatchMediaMock)

    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <SettingProvider>{children}</SettingProvider>
    )

    renderHook(() => useSetting(), { wrapper })

    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })
})
