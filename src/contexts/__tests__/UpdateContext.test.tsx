import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useEffect } from 'react'
import type React from 'react'
import {
  checkForUpdate,
  downloadUpdate,
  getDownloadProgress,
  subscribeUpdateProgress,
  type DownloadEvent,
} from '@/api/updater'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateProvider } from '@/contexts/UpdateContext'
import { useUpdate } from '@/hooks/useUpdate'
import type { Settings } from '@/types/setting'

vi.mock('@/api/updater', () => ({
  checkForUpdate: vi.fn(),
  installUpdate: vi.fn(),
  downloadUpdate: vi.fn().mockResolvedValue(undefined),
  cancelDownload: vi.fn().mockResolvedValue(undefined),
  getDownloadProgress: vi.fn().mockResolvedValue({
    phase: 'idle',
    downloaded: 0,
    total: null,
    version: null,
    currentVersion: '0.0.0-test',
    body: null,
    date: null,
  }),
  getInstallKind: vi.fn().mockResolvedValue('macos'),
  subscribeUpdateProgress: vi.fn(),
  subscribeUpdateAvailable: vi.fn().mockResolvedValue(() => {}),
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
    autoDownloadUpdate: false,
    theme: 'system',
    themeColor: null,
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

/**
 * Auto-trigger a default `checkForUpdates()` on mount. Mirrors the typical
 * call site (sidebar icon / "check for update" button) so we can exercise
 * state transitions without relying on the now-removed startup auto-check.
 */
const AutoCheckOnMountConsumer = () => {
  const { checkForUpdates, state } = useUpdate()
  useEffect(() => {
    void checkForUpdates()
  }, [checkForUpdates])
  return (
    <div>
      <span data-testid="phase">{state.phase}</span>
      <span data-testid="downloaded">{state.downloaded}</span>
      <span data-testid="total">{state.total ?? 'null'}</span>
      <span data-testid="version">{state.info?.version ?? 'none'}</span>
    </div>
  )
}

const StateConsumer = () => {
  const { state } = useUpdate()
  return (
    <div>
      <span data-testid="phase">{state.phase}</span>
      <span data-testid="downloaded">{state.downloaded}</span>
      <span data-testid="total">{state.total ?? 'null'}</span>
      <span data-testid="version">{state.info?.version ?? 'none'}</span>
    </div>
  )
}

const ManualAlphaCheckConsumer = () => {
  const { checkForUpdates } = useUpdate()

  return (
    <button type="button" onClick={() => void checkForUpdates('alpha')}>
      check alpha
    </button>
  )
}

function renderWithSetting(setting: Settings, children: React.ReactNode) {
  return render(
    <SettingContext.Provider
      value={{
        setting,
        loading: false,
        error: null,
        reloadSetting: vi.fn(),
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
      }}
    >
      <UpdateProvider>{children}</UpdateProvider>
    </SettingContext.Provider>
  )
}

describe('UpdateProvider', () => {
  const checkForUpdateMock = vi.mocked(checkForUpdate)
  const downloadUpdateMock = vi.mocked(downloadUpdate)
  const getDownloadProgressMock = vi.mocked(getDownloadProgress)
  const subscribeUpdateProgressMock = vi.mocked(subscribeUpdateProgress)

  beforeEach(() => {
    checkForUpdateMock.mockReset()
    downloadUpdateMock.mockReset()
    downloadUpdateMock.mockResolvedValue(undefined)
    getDownloadProgressMock.mockReset()
    getDownloadProgressMock.mockResolvedValue({
      phase: 'idle',
      downloaded: 0,
      total: null,
      version: null,
      currentVersion: '0.0.0-test',
      body: null,
      date: null,
    })
    subscribeUpdateProgressMock.mockReset()
    subscribeUpdateProgressMock.mockImplementation(async () => () => {})
  })

  it('does not auto-check on startup (backend scheduler owns this)', async () => {
    checkForUpdateMock.mockResolvedValue({
      version: '0.1.1',
      currentVersion: '0.1.0',
      date: '2026-01-25T00:00:00Z',
      body: 'Bug fixes',
    })

    renderWithSetting(baseSetting, <StateConsumer />)

    // Give effects a chance to run.
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(checkForUpdateMock).not.toHaveBeenCalled()
  })

  it('transitions to "available" after a successful manual check', async () => {
    checkForUpdateMock.mockResolvedValue({
      version: '0.2.0',
      currentVersion: '0.1.0',
      body: null,
      date: null,
    })

    renderWithSetting(baseSetting, <AutoCheckOnMountConsumer />)

    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('available')
    })
    expect(screen.getByTestId('version').textContent).toBe('0.2.0')
  })

  it('syncs initial state from backend snapshot on mount', async () => {
    getDownloadProgressMock.mockResolvedValue({
      phase: 'downloading',
      downloaded: 512,
      total: 2048,
      version: '0.2.0',
      currentVersion: '0.1.0',
      body: null,
      date: null,
    })

    renderWithSetting(baseSetting, <StateConsumer />)

    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('downloading')
    })
    expect(screen.getByTestId('downloaded').textContent).toBe('512')
    expect(screen.getByTestId('total').textContent).toBe('2048')
  })

  it('updates state from broadcast download events', async () => {
    let listener: (event: DownloadEvent) => void = () => {}
    subscribeUpdateProgressMock.mockImplementation(async cb => {
      listener = cb
      return () => {}
    })
    checkForUpdateMock.mockResolvedValue({
      version: '0.2.0',
      currentVersion: '0.1.0',
      body: null,
      date: null,
    })

    renderWithSetting(baseSetting, <AutoCheckOnMountConsumer />)

    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('available')
    })

    act(() => listener({ event: 'Started', data: { contentLength: 4096 } }))
    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('downloading')
    })
    expect(screen.getByTestId('total').textContent).toBe('4096')

    act(() => listener({ event: 'Progress', data: { chunkLength: 1024 } }))
    await waitFor(() => {
      expect(screen.getByTestId('downloaded').textContent).toBe('1024')
    })

    act(() => listener({ event: 'Finished' }))
    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('ready')
    })
  })

  it('reverts to "available" on download Failed event', async () => {
    let listener: (event: DownloadEvent) => void = () => {}
    subscribeUpdateProgressMock.mockImplementation(async cb => {
      listener = cb
      return () => {}
    })
    checkForUpdateMock.mockResolvedValue({
      version: '0.2.0',
      currentVersion: '0.1.0',
      body: null,
      date: null,
    })

    renderWithSetting(baseSetting, <AutoCheckOnMountConsumer />)

    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('available')
    })

    act(() => listener({ event: 'Started', data: { contentLength: 1024 } }))
    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('downloading')
    })

    act(() => listener({ event: 'Failed', data: { error: 'boom' } }))
    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('available')
    })
    expect(screen.getByTestId('downloaded').textContent).toBe('0')
  })

  it('does not auto-download even when autoDownloadUpdate is on (backend scheduler owns this)', async () => {
    checkForUpdateMock.mockResolvedValue({
      version: '0.2.0',
      currentVersion: '0.1.0',
      body: null,
      date: null,
    })
    const autoDownloadOn: Settings = {
      ...baseSetting,
      general: { ...baseSetting.general, autoDownloadUpdate: true },
    }

    renderWithSetting(autoDownloadOn, <AutoCheckOnMountConsumer />)

    await waitFor(() => {
      expect(screen.getByTestId('phase').textContent).toBe('available')
    })
    expect(downloadUpdateMock).not.toHaveBeenCalled()
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
          reloadSetting: vi.fn(),
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
