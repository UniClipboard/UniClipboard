import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSearchStatus } from '@/api/daemon'
import * as storageApi from '@/api/storage'
import StorageSection, { resolveMaxItemsCount } from '@/components/setting/StorageSection'
import { useSetting } from '@/hooks/useSetting'
import i18n from '@/i18n'
import type { Settings, SettingContextType } from '@/types/setting'

// ============================================================================
// Mock chain — isolate StorageSection from the real daemon HTTP client and
// Tauri runtime (ConfigBackupGroup, a StorageSection child, reaches into
// `@/lib/ipc` + `@/components/ui/toast` too).
// ============================================================================

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/api/daemon', () => ({
  getSearchStatus: vi.fn(),
  triggerSearchRebuild: vi.fn(),
}))

vi.mock('@/api/storage', () => ({
  getStorageStats: vi.fn(),
  clearCache: vi.fn(),
  openDataDirectory: vi.fn(),
  revealPath: vi.fn(),
  clearAllClipboardHistory: vi.fn(),
}))

vi.mock('@/lib/ipc', () => ({
  commands: {
    exportConfigPackage: vi.fn(),
    pickConfigBundlePath: vi.fn(),
    previewConfigImport: vi.fn(),
    importConfigPackage: vi.fn(),
    restartDaemon: vi.fn(),
    restartApp: vi.fn(),
  },
}))

vi.mock('@/components/ui/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    message: vi.fn(),
  },
}))

// ClearHistoryDialog (rendered unconditionally, closed) registers shortcut
// bindings that need a ShortcutProvider ancestor — stub them out like other
// settings tests do (see AboutSection.test.tsx, DeleteConfirmDialog.test.tsx).
vi.mock('@/hooks/useShortcutLayer', () => ({
  useShortcutLayer: vi.fn(),
}))
vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: vi.fn(),
}))

const mockUseSetting = vi.mocked(useSetting)
const mockGetSearchStatus = vi.mocked(getSearchStatus)
const mockGetStorageStats = vi.mocked(storageApi.getStorageStats)

beforeAll(() => {
  // Radix Select needs pointer-capture + scrollIntoView shims under jsdom.
  if (!HTMLElement.prototype.hasPointerCapture) {
    Object.defineProperty(HTMLElement.prototype, 'hasPointerCapture', {
      value: () => false,
    })
  }
  if (!HTMLElement.prototype.setPointerCapture) {
    Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
      value: () => undefined,
    })
  }
  if (!HTMLElement.prototype.releasePointerCapture) {
    Object.defineProperty(HTMLElement.prototype, 'releasePointerCapture', {
      value: () => undefined,
    })
  }
  if (!HTMLElement.prototype.scrollIntoView) {
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      value: () => undefined,
    })
  }
})

// ============================================================================
// Test fixtures
// ============================================================================

const baseSetting: Settings = {
  schemaVersion: 1,
  general: {
    autoStart: false,
    silentStart: false,
    autoCheckUpdate: true,
    autoDownloadUpdate: false,
    theme: 'light',
    themeColor: 'zinc',
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
    syncOnRestore: false,
  },
  retentionPolicy: {
    enabled: true,
    rules: [{ byAge: { maxAge: 30 * 86400 } }, { byCount: { maxItems: 500 } }],
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
  keyboardShortcuts: {},
  fileSync: {
    fileSyncEnabled: true,
    smallFileThreshold: 10 * 1024 * 1024,
    maxFileSize: 5 * 1024 * 1024 * 1024,
    fileCacheQuotaPerDevice: 500 * 1024 * 1024,
    fileRetentionHours: 24,
    fileAutoCleanup: true,
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

type UpdateRetentionPolicyFn = SettingContextType['updateRetentionPolicy']

interface SetupArgs {
  setting?: Settings | null
  error?: string | null
  updateRetentionPolicy?: ReturnType<typeof vi.fn<UpdateRetentionPolicyFn>>
}

const setupSetting = ({
  setting = baseSetting,
  error = null,
  updateRetentionPolicy,
}: SetupArgs = {}) => {
  const mockUpdate =
    updateRetentionPolicy ?? vi.fn<UpdateRetentionPolicyFn>().mockResolvedValue(undefined)
  mockUseSetting.mockReturnValue({
    setting,
    loading: false,
    error,
    reloadSetting: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: mockUpdate,
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
    updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })
  return { mockUpdate }
}

beforeAll(async () => {
  await i18n.changeLanguage('en-US')
})

beforeEach(() => {
  vi.clearAllMocks()
  mockGetStorageStats.mockResolvedValue({
    totalBytes: 100,
    databaseBytes: 25,
    vaultBytes: 25,
    cacheBytes: 25,
    logsBytes: 25,
  })
  mockGetSearchStatus.mockResolvedValue({
    data: {
      state: 'ready',
      reason: null,
      lastRebuildStartedAtMs: null,
      lastRebuildCompletedAtMs: null,
    },
    ts: Date.now(),
  })
})

afterEach(() => {
  cleanup()
})

// ============================================================================
// Regression coverage for the "unlimited" MAX_ITEMS_OPTIONS nullish-coalescing
// bug: `MAX_ITEMS_OPTIONS.find(...)?.count ?? 500` silently turned a found
// `count: null` (the "unlimited" option) back into 500, because `??` treats
// `null` as nullish. `resolveMaxItemsCount` is the fix, extracted so it can be
// unit-tested directly.
// ============================================================================
describe('resolveMaxItemsCount', () => {
  it('resolves "unlimited" to null (regression: used to fall back to 500)', () => {
    expect(resolveMaxItemsCount('unlimited')).toBeNull()
  })

  it('resolves a known numeric option to its count', () => {
    expect(resolveMaxItemsCount('500')).toBe(500)
  })

  it('falls back to 500 for a value that matches no known option', () => {
    expect(resolveMaxItemsCount('garbage-unknown-value')).toBe(500)
  })
})

describe('StorageSection — max items "unlimited" selection', () => {
  it('selecting "Unlimited" persists rules without a byCount entry', async () => {
    const user = userEvent.setup()
    const { mockUpdate } = setupSetting()

    render(<StorageSection />)

    // Radix's `combobox` role doesn't compute an accessible name from content
    // (that role isn't in the ARIA "name from content" set), and SettingRow
    // doesn't wire a <label for>/aria-labelledby to the Select — so we can't
    // filter by `name`. Fall back to DOM order, which mirrors the fixed JSX
    // order in StorageSection.tsx: retention-days select first, max-items
    // select second.
    const comboboxes = await screen.findAllByRole('combobox')
    expect(comboboxes).toHaveLength(2)
    const maxItemsCombobox = comboboxes[1]
    expect(maxItemsCombobox).toHaveTextContent('500 items')
    await user.click(maxItemsCombobox)
    await user.click(await screen.findByRole('option', { name: 'Unlimited' }))

    await waitFor(() => {
      expect(mockUpdate).toHaveBeenCalled()
    })

    const [call] = mockUpdate.mock.calls
    const newRules = (call?.[0] as { rules?: unknown[] } | undefined)?.rules
    expect(newRules).toBeDefined()
    expect(
      newRules?.some(rule => typeof rule === 'object' && rule !== null && 'byCount' in rule)
    ).toBe(false)
  })
})
