import type { Settings } from '@/api/daemon/settings'

/**
 * Baseline `Settings` fixture shared by component/context tests.
 *
 * Deliberately separate from `src/__tests__/api/daemon/_test-helpers.ts`'s
 * `makeSettingsDto`: that module also installs a `vi.mock('@/api/daemon/client')`
 * side effect on import (needed for the daemon API integration tests it serves),
 * which would leak into component/context tests that mock the daemon client
 * differently. This fixture has no side effects — safe to import anywhere.
 *
 * `overrides.general` is merged one level deep (the most commonly customized
 * section across these tests); every other top-level section is replaced
 * wholesale when provided in `overrides`.
 */
type BaseSettingsOverrides = Partial<Omit<Settings, 'general'>> & {
  general?: Partial<Settings['general']>
}

export function makeBaseSettings(overrides: BaseSettingsOverrides = {}): Settings {
  const { general: generalOverrides, ...rest } = overrides
  return {
    schemaVersion: 1,
    general: {
      autoStart: false,
      startupMode: 'normal',
      restoreLastEntryOnStartup: false,
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
      updateChannel: null,
      telemetryEnabled: true,
      usageAnalyticsEnabled: true,
      debugMode: false,
      ...generalOverrides,
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
      doubleTapModifier: 'disabled',
    },
    ...rest,
  }
}
