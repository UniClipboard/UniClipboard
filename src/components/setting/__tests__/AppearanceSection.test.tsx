import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import AppearanceSection from '@/components/setting/AppearanceSection'
import { useSetting } from '@/hooks/useSetting'
import { useUiScale } from '@/hooks/useUiScale'
import type { SettingContextType, Settings } from '@/types/setting'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/hooks/useUiScale', () => ({
  useUiScale: vi.fn(),
}))

vi.mock('@/lib/theme-transition', () => ({
  setTransitionOrigin: vi.fn(),
}))

const mockUseSetting = vi.mocked(useSetting)
const mockUseUiScale = vi.mocked(useUiScale)

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
    language: 'zh-CN',
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
  },
}

const setup = (theme: Settings['general']['theme'] = 'light') => {
  const updateGeneralSetting = vi
    .fn<SettingContextType['updateGeneralSetting']>()
    .mockResolvedValue(undefined)

  mockUseSetting.mockReturnValue({
    setting: {
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme,
      },
    },
    loading: false,
    error: null,
    updateSetting: vi.fn(),
    updateGeneralSetting,
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })

  mockUseUiScale.mockReturnValue({
    scale: 1,
    scalePercent: '100%',
    options: [{ value: 1, label: '100%' }],
    setScale: vi.fn(),
    resetScale: vi.fn(),
    isDefault: true,
    isSelected: option => option.value === 1,
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    canZoomIn: false,
    canZoomOut: false,
  })

  render(<AppearanceSection />)

  return { updateGeneralSetting }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('AppearanceSection', () => {
  it('把跟随系统显示为主题卡片而不是开关', () => {
    setup('system')

    expect(screen.queryByRole('switch')).toBeNull()
    expect(
      screen.getByRole('button', {
        name: 'settings.sections.appearance.themePreview.followSystem',
      })
    ).toHaveAttribute('aria-pressed', 'true')
  })

  it('点击跟随系统卡片会切换到系统模式', async () => {
    const user = userEvent.setup()
    const { updateGeneralSetting } = setup('light')

    await user.click(
      screen.getByRole('button', {
        name: 'settings.sections.appearance.themePreview.followSystem',
      })
    )

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ theme: 'system' })
    })
  })
})
