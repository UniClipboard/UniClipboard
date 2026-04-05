import { render, screen, within } from '@testing-library/react'
import AppearanceSection from '@/components/setting/AppearanceSection'
import { DEFAULT_THEME_COLOR, THEME_COLORS } from '@/constants/theme'
import { SettingContext } from '@/contexts/setting-context'
import type { Settings } from '@/types/setting'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    theme: 'system',
    themeColor: DEFAULT_THEME_COLOR,
    language: 'en-US',
    deviceName: 'Test Device',
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
}

function renderAppearanceSection() {
  return render(
    <SettingContext.Provider
      value={{
        setting: baseSetting,
        loading: false,
        error: null,
        updateSetting: vi.fn(),
        updateGeneralSetting: vi.fn(),
        updateSyncSetting: vi.fn(),
        updateSecuritySetting: vi.fn(),
        updateRetentionPolicy: vi.fn(),
        updateKeyboardShortcuts: vi.fn(),
        updateFileSyncSetting: vi.fn(),
      }}
    >
      <AppearanceSection />
    </SettingContext.Provider>
  )
}

describe('AppearanceSection - theme color swatches', () => {
  it('renders a swatch for each theme with 3-4 preview dots', () => {
    renderAppearanceSection()

    const swatches = screen.getAllByTestId('theme-color-swatch')
    expect(swatches).toHaveLength(THEME_COLORS.length)

    for (const swatch of swatches) {
      const dots = within(swatch).getAllByTestId('theme-color-dot')
      expect(dots.length).toBeGreaterThanOrEqual(3)
      expect(dots.length).toBeLessThanOrEqual(4)
    }
  })

  it('marks the default theme as selected when themeColor is unset', () => {
    render(
      <SettingContext.Provider
        value={{
          setting: { ...baseSetting, general: { ...baseSetting.general, themeColor: null } },
          loading: false,
          error: null,
          updateSetting: vi.fn(),
          updateGeneralSetting: vi.fn(),
          updateSyncSetting: vi.fn(),
          updateSecuritySetting: vi.fn(),
          updateRetentionPolicy: vi.fn(),
          updateKeyboardShortcuts: vi.fn(),
          updateFileSyncSetting: vi.fn(),
        }}
      >
        <AppearanceSection />
      </SettingContext.Provider>
    )

    const defaultLabel = screen.getByText(DEFAULT_THEME_COLOR)
    const defaultSwatch = defaultLabel.closest('[data-testid="theme-color-swatch"]')
    expect(defaultSwatch).not.toBeNull()
    expect(defaultSwatch).toHaveClass('border-primary')
  })
})
