import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings } from '@/api/daemon'
import type { Settings } from '@/api/daemon/settings'
import { DEFAULT_THEME_COLOR } from '@/constants/theme'
import { SettingProvider } from '@/contexts/SettingContext'
import { useSetting } from '@/hooks/useSetting'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/api/daemon', () => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}))

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn(),
}))

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

vi.mock('@/i18n', () => ({
  __esModule: true,
  default: {
    language: 'en-US',
    changeLanguage: vi.fn().mockResolvedValue(undefined),
  },
  normalizeLanguage: vi.fn((language: string | null | undefined) => language ?? 'en-US'),
  persistLanguage: vi.fn(),
}))

const mockGetSettings = vi.mocked(getSettings)
const mockConnectDaemonWs = vi.mocked(connectDaemonWs)
const mockInvokeWithTrace = vi.mocked(invokeWithTrace)

const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    autoDownloadUpdate: false,
    theme: 'light',
    themeColor: DEFAULT_THEME_COLOR,
    themeColorLight: null,
    themeColorDark: null,
    themeOverridesLight: {},
    themeOverridesDark: {},
    language: 'en-US',
    deviceName: 'Test Device',
    telemetryEnabled: true,
    usageAnalyticsEnabled: true,
    debugMode: false,
  },
  sync: {
    autoSync: true,
    syncFrequency: 'realtime',
    contentTypes: {
      text: true,
      image: true,
      link: true,
      file: true,
      codeSnippet: true,
      richText: true,
    },
  },
  retentionPolicy: {
    enabled: false,
    rules: [],
    skipPinned: false,
    evaluation: 'anyMatch',
  },
  security: {
    encryptionEnabled: false,
    passphraseConfigured: false,
    autoUnlockEnabled: false,
  },
  pairing: {
    stepTimeout: 15,
    userVerificationTimeout: 120,
    sessionTimeout: 300,
    maxRetries: 3,
    protocolVersion: '1.0.0',
  },
  keyboardShortcuts: {},
  fileSync: {
    fileSyncEnabled: true,
    smallFileThreshold: 10 * 1024 * 1024,
    maxFileSize: 5 * 1024 * 1024 * 1024,
    fileCacheQuotaPerDevice: 500 * 1024 * 1024,
    fileRetentionHours: 24,
    fileAutoCleanup: true,
  },
  network: {
    allowRelayFallback: true,
    allowOverlayNetworkAddrs: false,
    customRelayUrls: [],
  },
  quickPanel: {
    enabled: true,
    position: 'center',
  },
}

describe('SettingProvider theme integration', () => {
  let prefersDark = false

  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <SettingProvider>{children}</SettingProvider>
  )

  beforeEach(() => {
    vi.clearAllMocks()
    prefersDark = false
    mockConnectDaemonWs.mockResolvedValue(undefined)
    mockInvokeWithTrace.mockResolvedValue(undefined)
    mockGetSettings.mockResolvedValue(baseSetting)

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        get matches() {
          return prefersDark
        },
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    })
  })

  afterEach(() => {
    document.documentElement.className = ''
    document.documentElement.removeAttribute('data-theme')
  })

  it('applies persisted themeColor on mount', async () => {
    const { result } = renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(result.current.setting?.general.themeColor).toBe(DEFAULT_THEME_COLOR)
      expect(document.documentElement.getAttribute('data-theme')).toBe(DEFAULT_THEME_COLOR)
    })
  })

  it('falls back to the default preset when themeColor is null', async () => {
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        themeColor: null,
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(document.documentElement.getAttribute('data-theme')).toBe(DEFAULT_THEME_COLOR)
    })
  })

  it('applies the dark mode class when system theme is dark', async () => {
    prefersDark = true
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'system',
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(document.documentElement.classList.contains('dark')).toBe(true)
    })
  })

  it('applies themeColorDark preset when resolved mode is dark', async () => {
    prefersDark = true
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'system',
        themeColor: null,
        themeColorLight: 'zinc',
        themeColorDark: 'claude',
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(document.documentElement.getAttribute('data-theme')).toBe('claude')
    })
  })

  it('applies themeColorLight preset when resolved mode is light', async () => {
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'light',
        themeColor: null,
        themeColorLight: 'zinc',
        themeColorDark: 'claude',
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(document.documentElement.getAttribute('data-theme')).toBe('zinc')
    })
  })

  it('falls back to legacy themeColor when split fields are null', async () => {
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'light',
        themeColor: 'catppuccin',
        themeColorLight: null,
        themeColorDark: null,
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(document.documentElement.getAttribute('data-theme')).toBe('catppuccin')
    })
  })

  it('applies user override on top of preset for current mode', async () => {
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'light',
        themeColor: null,
        themeColorLight: 'zinc',
        themeColorDark: 'zinc',
        themeOverridesLight: {
          primary: 'oklch(0.5 0.2 270)',
        },
        themeOverridesDark: {
          background: 'oklch(0.18 0.02 280)',
        },
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      // light 模式应用了 primary override
      expect(document.documentElement.style.getPropertyValue('--primary')).toBe(
        'oklch(0.5 0.2 270)'
      )
      // dark 模式的 override 不应在 light 模式生效
      expect(document.documentElement.style.getPropertyValue('--background')).not.toBe(
        'oklch(0.18 0.02 280)'
      )
    })
  })

  it('ignores override keys outside the allow list', async () => {
    mockGetSettings.mockResolvedValue({
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'light',
        themeColor: null,
        themeColorLight: 'zinc',
        themeColorDark: 'zinc',
        themeOverridesLight: {
          // 非法 key, 防御逻辑应忽略
          'malicious-token': 'oklch(0.5 0.2 270)',
          // 合法 key
          primary: 'oklch(0.6 0.15 30)',
        },
        themeOverridesDark: {},
      },
    })

    renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue('--primary')).toBe(
        'oklch(0.6 0.15 30)'
      )
      // 非法 key 不会被写到任意 CSS 变量
      expect(document.documentElement.style.getPropertyValue('--malicious-token')).toBe('')
    })
  })
})
