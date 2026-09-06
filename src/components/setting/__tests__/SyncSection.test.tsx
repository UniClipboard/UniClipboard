import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import SyncSection from '@/components/setting/SyncSection'
import { useSetting } from '@/hooks/useSetting'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType } from '@/types/setting'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/lib/ipc', () => ({
  commands: { pickDirectory: vi.fn() },
}))

const mockUseSetting = vi.mocked(useSetting)

function setup({ syncEnabled = true, autoSyncEnabled = false } = {}) {
  const base = makeBaseSettings()
  const setting = {
    ...base,
    sync: {
      ...base.sync,
      syncEnabled,
      autoSyncEnabled,
    },
  }

  mockUseSetting.mockReturnValue({
    setting,
    loading: false,
    error: null,
    reloadSetting: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi
      .fn<SettingContextType['updateSyncSetting']>()
      .mockResolvedValue(undefined),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi
      .fn<SettingContextType['updateFileSyncSetting']>()
      .mockResolvedValue(undefined),
    updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
    saveRelay: vi.fn().mockResolvedValue({
      restartRequired: false,
      credentialStatus: { configured: false },
    }),
    updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })

  return render(<SyncSection />)
}

beforeEach(() => {
  vi.clearAllMocks()
})

afterEach(() => {
  cleanup()
})

describe('SyncSection', () => {
  it('keeps file sync available when automatic sync is disabled', () => {
    const { container } = setup()

    expect(container.querySelector('#auto-sync')).toHaveAttribute('aria-checked', 'false')
    expect(container.querySelector('#file-sync-enabled')).not.toBeDisabled()
  })

  it('disables file sync when synchronization is disabled', () => {
    const { container } = setup({ syncEnabled: false })

    expect(container.querySelector('#file-sync-enabled')).toBeDisabled()
  })

  it('rejects invalid beUI input and saves valid file sizes in bytes', async () => {
    setup()
    const input = screen.getByRole('textbox', {
      name: 'settings.sections.sync.fileSync.smallFileThreshold.label',
    })
    const update = mockUseSetting.mock.results[0].value.updateFileSyncSetting

    fireEvent.change(input, { target: { value: '0' } })
    expect(input).toHaveAttribute('aria-invalid', 'true')
    expect(screen.getByRole('alert')).toHaveTextContent(
      'settings.sections.sync.fileSync.smallFileThreshold.errors.range'
    )
    expect(update).not.toHaveBeenCalled()

    fireEvent.change(input, { target: { value: '20' } })
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith({ smallFileThreshold: 20 * 1024 * 1024 })
    )
    expect(input).not.toHaveAttribute('aria-invalid')
  })

  it('preserves cache and retention units through beUI inputs', async () => {
    setup()
    const update = mockUseSetting.mock.results[0].value.updateFileSyncSetting
    fireEvent.change(
      screen.getByRole('textbox', {
        name: 'settings.sections.sync.fileSync.cacheQuota.label',
      }),
      { target: { value: '750' } }
    )
    fireEvent.change(
      screen.getByRole('textbox', {
        name: 'settings.sections.sync.fileSync.retentionPeriod.label',
      }),
      { target: { value: '48' } }
    )
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith({ fileCacheQuotaPerDevice: 750 * 1024 * 1024 })
      expect(update).toHaveBeenCalledWith({ fileRetentionHours: 48 })
    })
  })
})
