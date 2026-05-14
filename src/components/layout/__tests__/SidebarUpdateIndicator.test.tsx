import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import type { UpdateMetadata } from '@/api/updater'
import Sidebar from '@/components/layout/Sidebar'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateContext, type UpdateContextType, type UpdateState } from '@/contexts/update-context'
import type { Settings } from '@/types/setting'

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

const updateInfo: UpdateMetadata = {
  version: '0.1.1',
  currentVersion: '0.1.0',
  date: '2026-01-25T00:00:00Z',
  body: 'Bug fixes',
}

function buildUpdateValue(state: UpdateState): UpdateContextType {
  return {
    state,
    updateInfo: state.info,
    downloadProgress: {
      phase: state.phase,
      downloaded: state.downloaded,
      total: state.total,
    },
    isCheckingUpdate: false,
    checkForUpdates: vi.fn().mockResolvedValue(null),
    downloadUpdate: vi.fn().mockResolvedValue(undefined),
    cancelDownload: vi.fn().mockResolvedValue(undefined),
    installUpdate: vi.fn().mockResolvedValue(undefined),
  }
}

function renderSidebar(state: UpdateState) {
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
        updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
      }}
    >
      <UpdateContext.Provider value={buildUpdateValue(state)}>
        <MemoryRouter>
          <Sidebar />
        </MemoryRouter>
      </UpdateContext.Provider>
    </SettingContext.Provider>
  )
}

describe('Sidebar update indicator', () => {
  it('shows the amber "available" icon when an update is available', async () => {
    renderSidebar({
      phase: 'available',
      info: updateInfo,
      downloaded: 0,
      total: null,
    })

    const button = await waitFor(() => screen.getByLabelText(/update available/i))
    expect(button).toHaveAttribute('data-update-state', 'available')
  })

  it('hides the icon when there is no update', () => {
    renderSidebar({ phase: 'idle', info: null, downloaded: 0, total: null })

    expect(screen.queryByLabelText(/update available/i)).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/downloading update/i)).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/update ready/i)).not.toBeInTheDocument()
  })

  it('shows the "downloading" indicator with progress text when downloading', async () => {
    renderSidebar({
      phase: 'downloading',
      info: updateInfo,
      downloaded: 512,
      total: 1024,
    })

    const button = await waitFor(() => screen.getByLabelText(/downloading update.*50/i))
    expect(button).toHaveAttribute('data-update-state', 'downloading')
  })

  it('shows the "downloading" indicator without percent when total is unknown', async () => {
    renderSidebar({
      phase: 'downloading',
      info: updateInfo,
      downloaded: 512,
      total: null,
    })

    const button = await waitFor(() => screen.getByLabelText(/downloading update/i))
    expect(button).toHaveAttribute('data-update-state', 'downloading')
    expect(button).not.toHaveTextContent(/%/)
  })

  it('shows the emerald "ready" indicator when the update has been downloaded', async () => {
    renderSidebar({
      phase: 'ready',
      info: updateInfo,
      downloaded: 1024,
      total: 1024,
    })

    const button = await waitFor(() => screen.getByLabelText(/update ready/i))
    expect(button).toHaveAttribute('data-update-state', 'ready')
  })
})
