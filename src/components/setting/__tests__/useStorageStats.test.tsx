import { act, cleanup, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { getStorageStats, type StorageStats } from '@/api/storage'
import { useStorageStats } from '../useStorageStats'

vi.mock('@/api/storage', () => ({ getStorageStats: vi.fn() }))
afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})
const stats: StorageStats = {
  totalBytes: 10,
  databaseBytes: 4,
  vaultBytes: 3,
  cacheBytes: 2,
  logsBytes: 1,
}

describe('storage statistics refresh', () => {
  it('does not replace refreshed statistics with an older slow response', async () => {
    let finish!: (value: StorageStats) => void
    vi.mocked(getStorageStats)
      .mockImplementationOnce(
        () =>
          new Promise(resolve => {
            finish = resolve
          })
      )
      .mockResolvedValue({ ...stats, totalBytes: 5 })
    const { result } = renderHook(useStorageStats)
    await act(async () => {
      await result.current.refresh()
    })
    await act(async () => finish(stats))
    expect(result.current.stats?.totalBytes).toBe(5)
    expect(result.current.loading).toBe(false)
  })

  it('does not refresh after the storage page is closed', async () => {
    vi.mocked(getStorageStats).mockResolvedValue(stats)
    const { result, unmount } = renderHook(useStorageStats)
    await act(async () => {})
    const refresh = result.current.refresh
    unmount()
    await refresh()
    expect(getStorageStats).toHaveBeenCalledTimes(1)
  })
})
