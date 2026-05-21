import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings, updateSettings } from '@/api/daemon'
import type { Settings } from '@/api/daemon/settings'
import { updateKeyboardShortcuts as persistKeyboardShortcuts } from '@/api/tauri-command'
import { SettingProvider } from '@/contexts/SettingContext'
import { useSetting } from '@/hooks/useSetting'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { emitSettingsChanged } from '@/lib/settings-events'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/api/daemon', () => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}))

vi.mock('@/api/tauri-command', () => ({
  updateKeyboardShortcuts: vi.fn(),
}))

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn(),
}))

vi.mock('@/lib/settings-events', () => ({
  emitSettingsChanged: vi.fn(),
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
const mockUpdateSettings = vi.mocked(updateSettings)
const mockPersistKeyboardShortcuts = vi.mocked(persistKeyboardShortcuts)
const mockConnectDaemonWs = vi.mocked(connectDaemonWs)
const mockEmitSettingsChanged = vi.mocked(emitSettingsChanged)
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
  keyboardShortcuts: {
    'global.toggleQuickPanel': 'meta+ctrl+v',
  },
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
  },
}

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <SettingProvider>{children}</SettingProvider>
)

describe('SettingContext shortcuts — in-process apply path', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockConnectDaemonWs.mockResolvedValue(undefined)
    mockGetSettings.mockResolvedValue(baseSetting)
    mockUpdateSettings.mockResolvedValue({ success: true, restartRequired: false })
    mockPersistKeyboardShortcuts.mockResolvedValue({
      'global.toggleQuickPanel': 'meta+shift+v',
    })
    mockEmitSettingsChanged.mockResolvedValue(undefined)
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

  it('更新快捷键时不走 daemon HTTP，避免丢失 OS 全局快捷键副作用', async () => {
    const { result } = renderHook(() => useSetting(), { wrapper })
    await waitFor(() => {
      expect(result.current.setting).toEqual(baseSetting)
    })

    await act(async () => {
      await result.current.updateKeyboardShortcuts({
        'global.toggleQuickPanel': 'meta+shift+v',
      })
    })

    expect(mockPersistKeyboardShortcuts).toHaveBeenCalledWith(
      {
        'global.toggleQuickPanel': 'meta+ctrl+v',
      },
      {
        'global.toggleQuickPanel': 'meta+shift+v',
      }
    )
    expect(mockUpdateSettings).not.toHaveBeenCalled()
    expect(result.current.setting?.keyboardShortcuts).toEqual({
      'global.toggleQuickPanel': 'meta+shift+v',
    })
    expect(mockEmitSettingsChanged).toHaveBeenCalledWith({
      ...baseSetting,
      keyboardShortcuts: {
        'global.toggleQuickPanel': 'meta+shift+v',
      },
    })
  })
})
