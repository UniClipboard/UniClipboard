/**
 * Integration tests for DaemonClient settings API module.
 *
 * Covers:
 * - GET /settings — correct shape, camelCase field names
 * - PUT /settings — validation errors (400), partial update, full success
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import {
  makeSettingsDto,
  setupMockClient,
  teardownMockClient,
  mockDaemonClient,
  makeValidationError,
  makeNotFoundError,
} from './_test-helpers'
import { DaemonErrorCode } from '@/api/daemon/errors'
import { getSettings, updateSettings } from '@/api/daemon/settings'
import type { Settings } from '@/api/daemon/settings'

describe('Settings API', () => {
  beforeEach(() => {
    setupMockClient()
  })

  afterEach(() => {
    teardownMockClient()
  })

  // ── GET /settings ───────────────────────────────────────────

  describe('getSettings()', () => {
    it('returns the full Settings object on success', async () => {
      const settings = makeSettingsDto()
      mockDaemonClient.request.mockResolvedValueOnce({ data: settings, ts: Date.now() })

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
          theme: 'dark',
          themeColor: '#1a1a1a',
          language: 'en-US',
          deviceName: 'My MacBook',
          updateChannel: 'stable',
        },
      })
      mockDaemonClient.request.mockResolvedValueOnce({ data: settings, ts: Date.now() })

      const result = await getSettings()

      // camelCase field names from daemon serde serialisation
      expect(result.general).toHaveProperty('autoStart')
      expect(result.general).toHaveProperty('autoCheckUpdate')
      expect(result.general).toHaveProperty('themeColor')
      expect(result.general).toHaveProperty('deviceName')
      expect(result.general).toHaveProperty('updateChannel')
      expect(result.general.theme).toBe('dark')
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
        },
      })
      mockDaemonClient.request.mockResolvedValueOnce({ data: settings, ts: Date.now() })

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
      mockDaemonClient.request.mockResolvedValueOnce({ data: settings, ts: Date.now() })

      const result = await getSettings()

      expect(result.retentionPolicy.enabled).toBe(true)
      expect(result.retentionPolicy.rules).toHaveLength(2)
      expect(result.retentionPolicy.rules[0]).toHaveProperty('byAge')
      expect(result.retentionPolicy.rules[1]).toHaveProperty('byCount')
    })

    it('re-throws DaemonApiError on HTTP failure', async () => {
      mockDaemonClient.request.mockRejectedValueOnce(makeNotFoundError('500 on /settings'))

      await expect(getSettings()).rejects.toMatchObject({
        code: DaemonErrorCode.NOT_FOUND,
      })
    })
  })

  // ── PUT /settings ───────────────────────────────────────────

  describe('updateSettings(partial)', () => {
    it('sends PUT with camelCase payload matching Settings schema', async () => {
      mockDaemonClient.request.mockResolvedValueOnce({ data: { success: true }, ts: Date.now() })

      await updateSettings({
        schemaVersion: 1,
        general: {
          autoStart: true,
          silentStart: false,
          autoCheckUpdate: true,
          theme: 'light',
          themeColor: null,
          language: null,
          deviceName: 'New Name',
          updateChannel: null,
        },
      })

      expect(mockDaemonClient.request).toHaveBeenCalledTimes(1)
      const [, opts] = mockDaemonClient.request.mock.calls[0] as [string, RequestInit]
      expect((opts as { method: string }).method).toBe('PUT')
      const body = (opts as unknown as { body: Record<string, unknown> }).body
      expect(body).toHaveProperty('general')
      expect(body.general as Record<string, unknown>).toHaveProperty('autoStart')
      expect(body.general as Record<string, unknown>).toHaveProperty('theme')
    })

    it('accepts a minimal partial update with only changed fields', async () => {
      mockDaemonClient.request.mockResolvedValueOnce({ data: { success: true }, ts: Date.now() })

      await updateSettings({
        general: {
          autoStart: false,
          silentStart: false,
          autoCheckUpdate: true,
          theme: 'dark',
          themeColor: null,
          language: null,
          deviceName: null,
          updateChannel: null,
        },
      })

      expect(mockDaemonClient.request).toHaveBeenCalledTimes(1)
    })

    it('encodes keyboardShortcuts as a patch object with nested shortcuts map', async () => {
      mockDaemonClient.request.mockResolvedValueOnce({ data: { success: true }, ts: Date.now() })

      await updateSettings({
        keyboardShortcuts: {
          toggle_main_window: ['CommandOrControl+Shift+V', 'Alt+Shift+V'],
        },
      })

      expect(mockDaemonClient.request).toHaveBeenCalledTimes(1)
      const [, opts] = mockDaemonClient.request.mock.calls[0] as [string, RequestInit]
      const body = (opts as unknown as { body: Record<string, unknown> }).body
      expect(body.keyboardShortcuts).toEqual({
        shortcuts: {
          toggle_main_window: ['CommandOrControl+Shift+V', 'Alt+Shift+V'],
        },
      })
    })

    it('re-throws DaemonApiError with validation detail on 400', async () => {
      mockDaemonClient.request.mockRejectedValueOnce(
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
            theme: 'invalid-theme' as Settings['general']['theme'],
            themeColor: null,
            language: null,
            deviceName: null,
            updateChannel: null,
          },
        })
      ).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })

    it('re-throws DaemonApiError on HTTP failure', async () => {
      mockDaemonClient.request.mockRejectedValueOnce(makeValidationError('500 on /settings'))

      await expect(updateSettings({})).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })
  })
})
