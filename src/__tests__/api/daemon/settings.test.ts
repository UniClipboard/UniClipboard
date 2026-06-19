/**
 * Integration tests for DaemonClient settings API module.
 *
 * Covers:
 * - GET /settings — correct shape, camelCase field names
 * - PUT /settings — validation errors (400), partial update, full success
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
// `./_test-helpers` 必须先于 `@/api/daemon/*` 加载: 它在 top-level 注册了
// `vi.mock('@/api/daemon/client', ...)`,只有先跑过才能保证 storage/settings
// 拿到的是被 mock 的 client; 一旦顺序反了,真实 client 会先进 ESM 缓存。
// eslint-disable-next-line import-x/order
import {
  makeSettingsDto,
  setupMockClient,
  teardownMockClient,
  makeValidationError,
  makeNotFoundError,
} from './_test-helpers'
import { DaemonErrorCode } from '@/api/daemon/errors'
import { getSettings, updateSettings } from '@/api/daemon/settings'
import type { Settings } from '@/api/daemon/settings'
import {
  getSettings as getSettingsSdk,
  updateSettings as updateSettingsSdk,
} from '@/api/generated/sdk.gen'

// ADR-008 P6: settings 走生成的 SDK + daemonClient.callSdk（见 _test-helpers 的
// callSdk 默认实现，忠实复刻真实 happy-path）。GET/PUT 的返回与错误都由这两个
// SDK fn mock 控制；callSdk 把 SDK fn 的 `{ data: <envelope> }` 透传给 wrapper。
vi.mock('@/api/generated/sdk.gen', () => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}))

const getSdkMock = getSettingsSdk as unknown as ReturnType<typeof vi.fn>
const updateSdkMock = updateSettingsSdk as unknown as ReturnType<typeof vi.fn>

describe('Settings API', () => {
  beforeEach(() => {
    setupMockClient()
    getSdkMock.mockReset()
    updateSdkMock.mockReset()
  })

  afterEach(() => {
    teardownMockClient()
  })

  // ── GET /settings ───────────────────────────────────────────

  describe('getSettings()', () => {
    it('returns the full Settings object on success', async () => {
      const settings = makeSettingsDto()
      getSdkMock.mockResolvedValueOnce({ data: { data: settings, ts: Date.now() } })

      const result = await getSettings()

      expect(result).toHaveProperty('schemaVersion')
      expect(result).toHaveProperty('general')
      expect(result).toHaveProperty('sync')
      expect(result).toHaveProperty('retentionPolicy')
      expect(result).toHaveProperty('security')
      expect(result).toHaveProperty('pairing')
      expect(result).toHaveProperty('keyboardShortcuts')
      expect(result).toHaveProperty('fileSync')
    })

    it('general settings use camelCase field names matching daemon serde', async () => {
      const settings = makeSettingsDto({
        general: {
          autoStart: true,
          silentStart: false,
          autoCheckUpdate: false,
          autoDownloadUpdate: false,
          theme: 'dark',
          themeColor: '#1a1a1a',
          themeColorLight: null,
          themeColorDark: null,
          themeOverridesLight: {},
          themeOverridesDark: {},
          language: 'en-US',
          deviceName: 'My MacBook',
          updateChannel: 'stable',
          telemetryEnabled: true,
          usageAnalyticsEnabled: false,
          debugMode: false,
        },
      })
      getSdkMock.mockResolvedValueOnce({ data: { data: settings, ts: Date.now() } })

      const result = await getSettings()

      // camelCase field names from daemon serde serialisation
      expect(result.general).toHaveProperty('autoStart')
      expect(result.general).toHaveProperty('autoCheckUpdate')
      expect(result.general).toHaveProperty('autoDownloadUpdate')
      expect(result.general).toHaveProperty('themeColor')
      expect(result.general).toHaveProperty('deviceName')
      expect(result.general).toHaveProperty('updateChannel')
      expect(result.general).toHaveProperty('usageAnalyticsEnabled')
      expect(result.general.theme).toBe('dark')
      expect(result.general.usageAnalyticsEnabled).toBe(false)
    })

    it('sync settings expose all content type toggles', async () => {
      const settings = makeSettingsDto({
        sync: {
          autoSync: true,
          syncFrequency: 'interval',
          contentTypes: {
            text: true,
            image: false,
            link: true,
            file: false,
            codeSnippet: true,
            richText: false,
          },
          syncOnRestore: false,
        },
      })
      getSdkMock.mockResolvedValueOnce({ data: { data: settings, ts: Date.now() } })

      const result = await getSettings()

      expect(result.sync.contentTypes.text).toBe(true)
      expect(result.sync.contentTypes.image).toBe(false)
      expect(result.sync.contentTypes.codeSnippet).toBe(true)
    })

    it('retention policy contains rules array with discriminated union types', async () => {
      const settings = makeSettingsDto({
        retentionPolicy: {
          enabled: true,
          rules: [{ byAge: { maxAge: 86400 * 30 } }, { byCount: { maxItems: 500 } }],
          skipPinned: true,
          evaluation: 'allMatch',
        },
      })
      getSdkMock.mockResolvedValueOnce({ data: { data: settings, ts: Date.now() } })

      const result = await getSettings()

      expect(result.retentionPolicy.enabled).toBe(true)
      expect(result.retentionPolicy.rules).toHaveLength(2)
      expect(result.retentionPolicy.rules[0]).toHaveProperty('byAge')
      expect(result.retentionPolicy.rules[1]).toHaveProperty('byCount')
    })

    it('re-throws DaemonApiError on HTTP failure', async () => {
      getSdkMock.mockRejectedValueOnce(makeNotFoundError('500 on /settings'))

      await expect(getSettings()).rejects.toMatchObject({
        code: DaemonErrorCode.NOT_FOUND,
      })
    })
  })

  // ── PUT /settings ───────────────────────────────────────────

  describe('updateSettings(partial)', () => {
    it('sends PUT with camelCase payload matching Settings schema', async () => {
      updateSdkMock.mockResolvedValueOnce({
        data: { data: { success: true, restartRequired: false }, ts: Date.now() },
      })

      await updateSettings({
        schemaVersion: 1,
        general: {
          autoStart: true,
          silentStart: false,
          autoCheckUpdate: true,
          autoDownloadUpdate: false,
          theme: 'light',
          themeColor: null,
          themeColorLight: null,
          themeColorDark: null,
          themeOverridesLight: {},
          themeOverridesDark: {},
          language: null,
          deviceName: 'New Name',
          updateChannel: null,
          telemetryEnabled: true,
          usageAnalyticsEnabled: true,
          debugMode: false,
        },
      })

      expect(updateSdkMock).toHaveBeenCalledTimes(1)
      const [opts] = updateSdkMock.mock.calls[0] as [{ body: Record<string, unknown> }]
      const body = opts.body
      expect(body).toHaveProperty('general')
      // autoStart is intentionally NOT sent through the daemon patch: the OS
      // launch-at-login registration is a desktop-host side effect the settings
      // pipeline does not perform. It is toggled via the dedicated
      // `update_autostart` command instead (see toSettingsPatchRequest).
      expect(body.general as Record<string, unknown>).not.toHaveProperty('autoStart')
      expect(body.general as Record<string, unknown>).toHaveProperty('theme')
      expect(body.general as Record<string, unknown>).toHaveProperty('usageAnalyticsEnabled', true)
    })

    it('accepts a minimal partial update with only changed fields', async () => {
      updateSdkMock.mockResolvedValueOnce({
        data: { data: { success: true, restartRequired: false }, ts: Date.now() },
      })

      await updateSettings({
        general: {
          autoStart: false,
          silentStart: false,
          autoCheckUpdate: true,
          autoDownloadUpdate: false,
          theme: 'dark',
          themeColor: null,
          themeColorLight: null,
          themeColorDark: null,
          themeOverridesLight: {},
          themeOverridesDark: {},
          language: null,
          deviceName: null,
          updateChannel: null,
          telemetryEnabled: true,
          usageAnalyticsEnabled: true,
          debugMode: false,
        },
      })

      expect(updateSdkMock).toHaveBeenCalledTimes(1)
    })

    it('encodes keyboardShortcuts as a patch object with nested shortcuts map', async () => {
      updateSdkMock.mockResolvedValueOnce({
        data: { data: { success: true, restartRequired: false }, ts: Date.now() },
      })

      await updateSettings({
        keyboardShortcuts: {
          toggle_main_window: ['CommandOrControl+Shift+V', 'Alt+Shift+V'],
        },
      })

      expect(updateSdkMock).toHaveBeenCalledTimes(1)
      const [opts] = updateSdkMock.mock.calls[0] as [{ body: Record<string, unknown> }]
      const body = opts.body
      expect(body.keyboardShortcuts).toEqual({
        shortcuts: {
          toggle_main_window: ['CommandOrControl+Shift+V', 'Alt+Shift+V'],
        },
      })
    })

    it('re-throws DaemonApiError with validation detail on 400', async () => {
      updateSdkMock.mockRejectedValueOnce(
        makeValidationError('field "theme" must be one of light|dark|system', {
          field: 'general.theme',
          constraint: 'enum',
        })
      )

      await expect(
        updateSettings({
          general: {
            autoStart: false,
            silentStart: false,
            autoCheckUpdate: true,
            autoDownloadUpdate: false,
            theme: 'invalid-theme' as Settings['general']['theme'],
            themeColor: null,
            themeColorLight: null,
            themeColorDark: null,
            themeOverridesLight: {},
            themeOverridesDark: {},
            language: null,
            deviceName: null,
            updateChannel: null,
            telemetryEnabled: true,
            usageAnalyticsEnabled: true,
            debugMode: false,
          },
        })
      ).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })

    it('re-throws DaemonApiError on HTTP failure', async () => {
      updateSdkMock.mockRejectedValueOnce(makeValidationError('500 on /settings'))

      await expect(updateSettings({})).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })
  })
})
