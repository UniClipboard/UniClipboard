import { emit } from '@tauri-apps/api/event'
import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings, updateSettings } from '@/api/daemon'
import type { Settings } from '@/api/daemon/settings'
import { SettingProvider } from '@/contexts/SettingContext'
import { useSetting } from '@/hooks/useSetting'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(),
}))

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

const mockEmit = vi.mocked(emit)
const mockGetSettings = vi.mocked(getSettings)
const mockUpdateSettings = vi.mocked(updateSettings)
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
    themeColor: 'zinc',
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
    congestionController: 'cubic',
  },
  quickPanel: {
    enabled: true,
    position: 'center',
  },
}

describe('SettingProvider cross-window sync', () => {
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <SettingProvider>{children}</SettingProvider>
  )

  beforeEach(() => {
    vi.clearAllMocks()
    mockConnectDaemonWs.mockResolvedValue(undefined)
    mockGetSettings.mockResolvedValue(baseSetting)
    mockUpdateSettings.mockResolvedValue({ success: true, restartRequired: false })
    mockInvokeWithTrace.mockResolvedValue(undefined)

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockReturnValue({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    })
  })

  it('broadcasts updated settings after a theme change so other windows can sync', async () => {
    const { result } = renderHook(() => useSetting(), { wrapper })

    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    const updatedSetting: Settings = {
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme: 'dark',
      },
    }

    await act(async () => {
      await result.current.updateGeneralSetting({ theme: 'dark' })
    })

    expect(mockUpdateSettings).toHaveBeenCalledWith(updatedSetting)
    expect(mockEmit).toHaveBeenCalledWith('settings://changed', {
      settingJson: JSON.stringify(updatedSetting),
      timestamp: expect.any(Number),
    })
  })
})
