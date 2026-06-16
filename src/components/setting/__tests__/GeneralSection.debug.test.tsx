import '@testing-library/jest-dom/vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { exportLogs, updateDebugMode } from '@/api/daemon/diagnostics'
import GeneralSection from '@/components/setting/GeneralSection'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import type { SettingContextType, Settings } from '@/types/setting'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/api/daemon/diagnostics', () => ({
  updateDebugMode: vi.fn(),
  exportLogs: vi.fn(),
}))

vi.mock('@/api/storage', () => ({
  openLogsDirectory: vi.fn(),
  revealPath: vi.fn(),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/lib/ipc', () => ({
  commands: {
    restartDaemon: vi.fn().mockResolvedValue(undefined),
    restartApp: vi.fn().mockResolvedValue(undefined),
  },
}))

vi.mock('@/components/ui/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    message: vi.fn(),
  },
}))

const mockUseSetting = vi.mocked(useSetting)
const mockUpdateDebugMode = vi.mocked(updateDebugMode)
const mockExportLogs = vi.mocked(exportLogs)
const mockRestartDaemon = vi.mocked(commands.restartDaemon)
const mockRestartApp = vi.mocked(commands.restartApp)

const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    autoDownloadUpdate: false,
    theme: 'system',
    themeColor: null,
    themeColorLight: null,
    themeColorDark: null,
    themeOverridesLight: {},
    themeOverridesDark: {},
    language: 'en-US',
    deviceName: 'Test Device',
    updateChannel: null,
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

function setup(setting: Settings = baseSetting) {
  const reloadSetting = vi.fn<SettingContextType['reloadSetting']>().mockResolvedValue(undefined)
  mockUseSetting.mockReturnValue({
    setting,
    loading: false,
    error: null,
    reloadSetting,
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
    updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })
  return { reloadSetting }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('GeneralSection debug diagnostics controls', () => {
  it('enables debug mode then restarts the daemon and app after confirmation', async () => {
    const user = userEvent.setup()
    mockUpdateDebugMode.mockResolvedValue({ debugMode: true, restartRequired: true })
    const { reloadSetting } = setup()

    render(<GeneralSection />)

    await user.click(screen.getByRole('switch', { name: /logs\.debug\.label/ }))
    expect(mockUpdateDebugMode).not.toHaveBeenCalled()
    expect(mockRestartDaemon).not.toHaveBeenCalled()
    expect(mockRestartApp).not.toHaveBeenCalled()

    await user.click(screen.getByRole('button', { name: /logs\.debug\.confirm$/ }))

    await waitFor(() => {
      expect(mockUpdateDebugMode).toHaveBeenCalledWith(true)
    })
    expect(reloadSetting).toHaveBeenCalledTimes(1)
    await waitFor(() => {
      expect(mockRestartDaemon).toHaveBeenCalledTimes(1)
    })
    expect(mockRestartApp).toHaveBeenCalledTimes(1)
  })

  it('exports the last 24 hours of logs and shows the path', async () => {
    const user = userEvent.setup()
    mockExportLogs.mockResolvedValue({
      path: '/home/test/Downloads/uniclipboard-debug-logs.zip',
      includedFiles: ['uniclipboard-daemon.json.2026-06-16'],
      since: '2026-06-15T00:00:00Z',
    })
    setup()

    render(<GeneralSection />)

    await user.click(
      screen.getByRole('button', {
        name: 'settings.sections.general.logs.export.button',
      })
    )

    await waitFor(() => {
      expect(mockExportLogs).toHaveBeenCalledWith(24)
    })
    expect(screen.getByText('/home/test/Downloads/uniclipboard-debug-logs.zip')).toBeInTheDocument()
  })
})
