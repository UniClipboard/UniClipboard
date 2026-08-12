import '@testing-library/jest-dom/vitest'
import { cleanup, render } from '@testing-library/react'
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
})
