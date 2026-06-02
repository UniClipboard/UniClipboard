/**
 * Integration tests for DaemonClient storage API module.
 *
 * Covers:
 * - GET /storage/stats — correct shape, values
 * - POST /storage/clear-cache — confirmed:true (success), error paths
 *
 * Transport (ADR-008 P7): `getStorageStats` / `clearCache` route through the
 * generated SDK via `daemonClient.callSdk`. The shared `_test-helpers` already
 * mocks `callSdk` to replay the happy path (`call().then(r => r.data)`), so the
 * SDK-fn mocks below resolve to `{ data: <envelope> }` and the payload flows
 * straight through.
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
// `./_test-helpers` 必须先于 `@/api/daemon/*` 加载: 它在 top-level 注册了
// `vi.mock('@/api/daemon/client', ...)`,只有先跑过才能保证 storage/settings
// 拿到的是被 mock 的 client; 一旦顺序反了,真实 client 会先进 ESM 缓存。
// eslint-disable-next-line import-x/order
import { setupMockClient, teardownMockClient, makeStorageStatsDto } from './_test-helpers'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { getStorageStats, clearCache } from '@/api/daemon/storage'
import { clearStorageCache, getStorageStats as getStorageStatsSdk } from '@/api/generated/sdk.gen'

vi.mock('@/api/generated/sdk.gen', () => ({
  getStorageStats: vi.fn(),
  clearStorageCache: vi.fn(),
}))

const getStorageStatsMock = vi.mocked(getStorageStatsSdk)
const clearStorageCacheMock = vi.mocked(clearStorageCache)

describe('Storage API', () => {
  beforeEach(() => {
    setupMockClient()
    getStorageStatsMock.mockReset()
    clearStorageCacheMock.mockReset()
  })

  afterEach(() => {
    teardownMockClient()
  })

  // ── GET /storage/stats ──────────────────────────────────────

  describe('getStorageStats()', () => {
    it('returns correct shape with all required fields', async () => {
      const dto = makeStorageStatsDto()
      getStorageStatsMock.mockResolvedValueOnce({ data: { data: dto, ts: 0 } } as never)

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
      getStorageStatsMock.mockResolvedValueOnce({ data: { data: dto, ts: 0 } } as never)

      const stats = await getStorageStats()

      expect(stats.totalBytes).toBe(0)
      expect(stats.databaseBytes).toBe(0)
    })

    it('calls the getStorageStats SDK fn with throwOnError', async () => {
      getStorageStatsMock.mockResolvedValueOnce({
        data: { data: makeStorageStatsDto(), ts: 0 },
      } as never)

      await getStorageStats()

      expect(getStorageStatsMock).toHaveBeenCalledWith({ throwOnError: true })
    })

    it('re-throws DaemonApiError on failure', async () => {
      getStorageStatsMock.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '500 on /storage/stats')
      )

      await expect(getStorageStats()).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })
  })

  // ── POST /storage/clear-cache ───────────────────────────────

  describe('clearCache(confirmed)', () => {
    it('sends POST with confirmed:true via the clearStorageCache SDK fn', async () => {
      clearStorageCacheMock.mockResolvedValueOnce({
        data: { data: { freedBytes: 0 }, ts: 0 },
      } as never)

      await clearCache(true)

      expect(clearStorageCacheMock).toHaveBeenCalledWith({
        body: { confirmed: true },
        throwOnError: true,
      })
    })

    it('sends confirmed:false when explicitly called with false', async () => {
      clearStorageCacheMock.mockResolvedValueOnce({
        data: { data: { freedBytes: 0 }, ts: 0 },
      } as never)

      await clearCache(false)

      expect(clearStorageCacheMock).toHaveBeenCalledWith({
        body: { confirmed: false },
        throwOnError: true,
      })
    })

    it('rejects with DaemonApiError on failure', async () => {
      clearStorageCacheMock.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'Missing required field: confirmed')
      )

      await expect(clearCache(false)).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })

    it('re-throws DaemonApiError on HTTP 500 failure', async () => {
      clearStorageCacheMock.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '500 on /storage/clear-cache')
      )

      await expect(clearCache(true)).rejects.toMatchObject({
        code: DaemonErrorCode.INTERNAL_ERROR,
      })
    })
  })
})
