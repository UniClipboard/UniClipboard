import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import type { UpdateMetadata } from '@/api/updater'
import Sidebar from '@/components/layout/Sidebar'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateContext } from '@/contexts/update-context'
import type { Settings } from '@/types/setting'

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
  network: {
    allowRelayFallback: true,
  },
}

describe('Sidebar update indicator', () => {
  it('shows update icon when updater returns update info', async () => {
    const updateInfo: UpdateMetadata = {
      version: '0.1.1',
      currentVersion: '0.1.0',
      date: '2026-01-25T00:00:00Z',
      body: 'Bug fixes',
    }

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
          updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
        }}
      >
        <UpdateContext.Provider
          value={{
            updateInfo,
            isCheckingUpdate: false,
            checkForUpdates: vi.fn(),
            installUpdate: vi.fn(),
            downloadProgress: { downloaded: 0, total: null, phase: 'idle' as const },
          }}
        >
          <MemoryRouter>
            <Sidebar />
          </MemoryRouter>
        </UpdateContext.Provider>
      </SettingContext.Provider>
    )

    await waitFor(() => {
      expect(screen.getByLabelText(/update available/i)).toBeInTheDocument()
    })
  })

  it('hides update icon when there is no update info', () => {
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
          updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
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
          <MemoryRouter>
            <Sidebar />
          </MemoryRouter>
        </UpdateContext.Provider>
      </SettingContext.Provider>
    )

    expect(screen.queryByLabelText(/update available/i)).not.toBeInTheDocument()
  })
})
