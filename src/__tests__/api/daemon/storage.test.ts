/**
 * Integration tests for DaemonClient storage API module.
 *
 * Covers:
 * - GET /storage/stats — correct shape, values
 * - POST /storage/clear-cache — confirmed:true (success), error paths
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it, beforeEach, afterEach } from 'vitest'
// `./_test-helpers` 必须先于 `@/api/daemon/*` 加载: 它在 top-level 注册了
// `vi.mock('@/api/daemon/client', ...)`,只有先跑过才能保证 storage/settings
// 拿到的是被 mock 的 client; 一旦顺序反了,真实 client 会先进 ESM 缓存。
// eslint-disable-next-line import-x/order
import {
  mockDaemonClient,
  setupMockClient,
  teardownMockClient,
  makeStorageStatsDto,
} from './_test-helpers'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { getStorageStats, clearCache } from '@/api/daemon/storage'

describe('Storage API', () => {
  beforeEach(() => {
    setupMockClient()
  })

  afterEach(() => {
    teardownMockClient()
  })

  // ── GET /storage/stats ──────────────────────────────────────

  describe('getStorageStats()', () => {
    it('returns correct shape with all required fields', async () => {
      const dto = makeStorageStatsDto()
      mockDaemonClient.request.mockResolvedValueOnce({ data: dto, ts: Date.now() })

      const stats = await getStorageStats()

      expect(stats).toHaveProperty('totalBytes')
      expect(stats).toHaveProperty('databaseBytes')
      expect(stats).toHaveProperty('vaultBytes')
      expect(stats).toHaveProperty('cacheBytes')
      expect(stats).toHaveProperty('logsBytes')
      expect(stats.totalBytes).toBe(dto.totalBytes)
      expect(stats.databaseBytes).toBe(dto.databaseBytes)
      expect(stats.vaultBytes).toBe(dto.vaultBytes)
      expect(stats.cacheBytes).toBe(dto.cacheBytes)
      expect(stats.logsBytes).toBe(dto.logsBytes)
    })

    it('returns zero values when storage is empty', async () => {
      const dto = makeStorageStatsDto({
        totalBytes: 0,
        databaseBytes: 0,
        vaultBytes: 0,
        cacheBytes: 0,
        logsBytes: 0,
      })
      mockDaemonClient.request.mockResolvedValueOnce({ data: dto, ts: Date.now() })

      const stats = await getStorageStats()

      expect(stats.totalBytes).toBe(0)
      expect(stats.databaseBytes).toBe(0)
    })

    it('calls /storage/stats endpoint', async () => {
      mockDaemonClient.request.mockResolvedValueOnce({
        data: makeStorageStatsDto(),
        ts: Date.now(),
      })

      await getStorageStats()

      expect(mockDaemonClient.request).toHaveBeenCalledWith('/storage/stats')
    })

    it('re-throws DaemonApiError on failure', async () => {
      mockDaemonClient.request.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '500 on /storage/stats')
      )

      await expect(getStorageStats()).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })
  })

  // ── POST /storage/clear-cache ───────────────────────────────

  describe('clearCache(confirmed)', () => {
    it('sends POST with confirmed:true to /storage/clear-cache', async () => {
      mockDaemonClient.request.mockResolvedValueOnce(undefined)

      await clearCache(true)

      expect(mockDaemonClient.request).toHaveBeenCalledWith('/storage/clear-cache', {
        method: 'POST',
        body: { confirmed: true },
      })
    })

    it('sends confirmed:false when explicitly called with false', async () => {
      mockDaemonClient.request.mockResolvedValueOnce(undefined)

      await clearCache(false)

      expect(mockDaemonClient.request).toHaveBeenCalledWith('/storage/clear-cache', {
        method: 'POST',
        body: { confirmed: false },
      })
    })

    it('rejects with DaemonApiError on failure', async () => {
      mockDaemonClient.request.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'Missing required field: confirmed')
      )

      await expect(clearCache(false)).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })

    it('re-throws DaemonApiError on HTTP 500 failure', async () => {
      mockDaemonClient.request.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '500 on /storage/clear-cache')
      )

      await expect(clearCache(true)).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })
  })
})
