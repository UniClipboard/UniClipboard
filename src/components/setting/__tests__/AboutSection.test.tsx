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

vi.mock('@/hooks/useShortcutLayer', () => ({
  useShortcutLayer: vi.fn(),
}))

const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    theme: 'system',
    themeColor: null,
    language: 'en-US',
    deviceName: 'Test Device',
    telemetryEnabled: true,
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
          updateKeyboardShortcuts: vi.fn(),
          updateFileSyncSetting: vi.fn(),
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
          updateKeyboardShortcuts: vi.fn(),
          updateFileSyncSetting: vi.fn(),
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
      expect(updateGeneralSetting).toHaveBeenCalledWith({ autoCheckUpdate: false })
    })
  })
})
