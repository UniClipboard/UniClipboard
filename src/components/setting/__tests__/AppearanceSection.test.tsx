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
  schema_version: 1,
  general: {
    auto_start: false,
    silent_start: false,
    auto_check_update: true,
    theme: 'system',
    theme_color: DEFAULT_THEME_COLOR,
    language: 'en-US',
    device_name: 'Test Device',
  },
  sync: {
    auto_sync: true,
    sync_frequency: 'realtime',
    content_types: {
      text: true,
      image: true,
      link: true,
      file: true,
      code_snippet: true,
      rich_text: true,
    },
    max_file_size_mb: 10,
  },
  retention_policy: {
    enabled: false,
    rules: [],
    skip_pinned: false,
    evaluation: 'any_match',
  },
  security: {
    encryption_enabled: false,
    passphrase_configured: false,
    auto_unlock_enabled: false,
  },
  pairing: {
    step_timeout: 15,
    user_verification_timeout: 120,
    session_timeout: 300,
    max_retries: 3,
    protocol_version: '1.0.0',
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

  it('marks the default theme as selected when theme_color is unset', () => {
    render(
      <SettingContext.Provider
        value={{
          setting: { ...baseSetting, general: { ...baseSetting.general, theme_color: null } },
          loading: false,
          error: null,
          updateSetting: vi.fn(),
          updateGeneralSetting: vi.fn(),
          updateSyncSetting: vi.fn(),
          updateSecuritySetting: vi.fn(),
          updateRetentionPolicy: vi.fn(),
          updateKeyboardShortcuts: vi.fn(),
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
