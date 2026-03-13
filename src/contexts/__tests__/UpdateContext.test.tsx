import { render, screen, waitFor } from '@testing-library/react'
import { checkForUpdate } from '@/api/updater'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateProvider } from '@/contexts/UpdateContext'
import { useUpdate } from '@/hooks/useUpdate'
import type { Settings } from '@/types/setting'

vi.mock('@/api/updater', () => ({
  checkForUpdate: vi.fn(),
  installUpdate: vi.fn(),
}))

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

const UpdateConsumer = () => {
  const { updateInfo } = useUpdate()
  return <div>{updateInfo?.version ?? 'none'}</div>
}

describe('UpdateProvider', () => {
  const checkForUpdateMock = vi.mocked(checkForUpdate)

  beforeEach(() => {
    checkForUpdateMock.mockReset()
  })

  it('checks for updates once on startup when enabled', async () => {
    checkForUpdateMock.mockResolvedValue({
      version: '0.1.1',
      currentVersion: '0.1.0',
      date: '2026-01-25T00:00:00Z',
      body: 'Bug fixes',
    })

    const { rerender } = render(
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
        <UpdateProvider>
          <UpdateConsumer />
        </UpdateProvider>
      </SettingContext.Provider>
    )

    await waitFor(() => {
      expect(checkForUpdateMock).toHaveBeenCalledTimes(1)
    })

    expect(screen.getByText('0.1.1')).toBeInTheDocument()

    rerender(
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
        <UpdateProvider>
          <UpdateConsumer />
        </UpdateProvider>
      </SettingContext.Provider>
    )

    await waitFor(() => {
      expect(checkForUpdateMock).toHaveBeenCalledTimes(1)
    })
  })

  it('skips auto check when disabled', async () => {
    const disabledSetting: Settings = {
      ...baseSetting,
      general: {
        ...baseSetting.general,
        auto_check_update: false,
      },
    }

    render(
      <SettingContext.Provider
        value={{
          setting: disabledSetting,
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
        <UpdateProvider>
          <UpdateConsumer />
        </UpdateProvider>
      </SettingContext.Provider>
    )

    await waitFor(() => {
      expect(checkForUpdateMock).not.toHaveBeenCalled()
    })
  })
})
