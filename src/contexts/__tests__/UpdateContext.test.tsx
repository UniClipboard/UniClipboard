import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
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
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    theme: 'system',
    themeColor: null,
    themeColorLight: null,
    themeColorDark: null,
    themeOverridesLight: {},
    themeOverridesDark: {},
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
  network: {
    allowRelayFallback: true,
    allowOverlayNetworkAddrs: false,
  },
}

const UpdateConsumer = () => {
  const { updateInfo } = useUpdate()
  return <div>{updateInfo?.version ?? 'none'}</div>
}

const ManualAlphaCheckConsumer = () => {
  const { checkForUpdates } = useUpdate()

  return (
    <button type="button" onClick={() => void checkForUpdates('alpha')}>
      check alpha
    </button>
  )
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
          updateFileSyncSetting: vi.fn(),
          updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
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
          updateFileSyncSetting: vi.fn(),
          updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
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
        autoCheckUpdate: false,
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
          updateFileSyncSetting: vi.fn(),
          updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
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

  it('uses explicit channel override for manual checks', async () => {
    const user = userEvent.setup()
    const disabledSetting: Settings = {
      ...baseSetting,
      general: {
        ...baseSetting.general,
        autoCheckUpdate: false,
        updateChannel: 'stable',
      },
    }

    checkForUpdateMock.mockResolvedValue(null)

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
          updateFileSyncSetting: vi.fn(),
          updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
        }}
      >
        <UpdateProvider>
          <ManualAlphaCheckConsumer />
        </UpdateProvider>
      </SettingContext.Provider>
    )

    await user.click(screen.getByRole('button', { name: 'check alpha' }))

    await waitFor(() => {
      expect(checkForUpdateMock).toHaveBeenCalledWith('alpha')
    })
  })
})
