import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import AboutSection from '@/components/setting/AboutSection'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateContext } from '@/contexts/update-context'
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
    theme_color: null,
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

describe('AboutSection', () => {
  it('runs update check when clicking the button', async () => {
    const checkForUpdates = vi.fn().mockResolvedValue({
      version: '0.1.1',
      currentVersion: '0.1.0',
      date: '2026-01-25T00:00:00Z',
      body: 'Bug fixes',
    })

    render(
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
        }}
      >
        <UpdateContext.Provider
          value={{
            updateInfo: null,
            isCheckingUpdate: false,
            checkForUpdates,
            installUpdate: vi.fn(),
            downloadProgress: { downloaded: 0, total: null, phase: 'idle' as const },
          }}
        >
          <AboutSection />
        </UpdateContext.Provider>
      </SettingContext.Provider>
    )

    await userEvent.click(
      screen.getByRole('button', { name: 'settings.sections.about.checkUpdate' })
    )

    await waitFor(() => {
      expect(checkForUpdates).toHaveBeenCalledTimes(1)
    })

    expect(screen.getByText('update.title')).toBeInTheDocument()
  })

  it('toggles auto update checks', async () => {
    const updateGeneralSetting = vi.fn().mockResolvedValue(undefined)

    render(
      <SettingContext.Provider
        value={{
          setting: baseSetting,
          loading: false,
          error: null,
          updateSetting: vi.fn(),
          updateGeneralSetting,
          updateSyncSetting: vi.fn(),
          updateSecuritySetting: vi.fn(),
          updateRetentionPolicy: vi.fn(),
        }}
      >
        <UpdateContext.Provider
          value={{
            updateInfo: null,
            isCheckingUpdate: false,
            checkForUpdates: vi.fn(),
            installUpdate: vi.fn(),
            downloadProgress: { downloaded: 0, total: null, phase: 'idle' as const },
          }}
        >
          <AboutSection />
        </UpdateContext.Provider>
      </SettingContext.Provider>
    )

    expect(screen.getByText('settings.sections.about.autoCheckUpdate.label')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('switch'))

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ auto_check_update: false })
    })
  })
})
