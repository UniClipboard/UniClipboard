import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  clearBlobImageCache,
  getBlobImageObjectUrl,
  historyImageCacheSize,
  peekBlobImageObjectUrl,
  releaseBlobImage,
  reserveSelectedBlobImage,
  retainBlobImage,
} from '@/api/daemon/blob-image-cache'
import { daemonClient } from '@/api/daemon/client'

vi.mock('@/api/daemon/client', () => ({ daemonClient: { fetchBlob: vi.fn() } }))

describe('history image cache budgets', () => {
  beforeEach(() => {
    clearBlobImageCache()
    vi.mocked(daemonClient.fetchBlob).mockResolvedValue(new Blob(['image']))
    let sequence = 0
    vi.stubGlobal(
      'URL',
      class extends URL {
        static createObjectURL = vi.fn(() => `blob:${++sequence}`)
        static revokeObjectURL = vi.fn()
      }
    )
  })
  afterEach(() => {
    clearBlobImageCache()
    vi.unstubAllGlobals()
  })

  it('reserves one selected URL in addition to 50 pinned window thumbnails', async () => {
    const releaseSelected = reserveSelectedBlobImage('/selected-detail')
    const selected = await getBlobImageObjectUrl('/selected-detail')
    for (let i = 0; i < 50; i++) {
      retainBlobImage(`/thumbnail-${i}`)
      await getBlobImageObjectUrl(`/thumbnail-${i}`)
      expect(historyImageCacheSize()).toBeLessThanOrEqual(51)
    }
    expect(historyImageCacheSize()).toBe(51)
    expect(peekBlobImageObjectUrl('/selected-detail')).toBe(selected)
    await expect(getBlobImageObjectUrl('/overflow')).rejects.toMatchObject({
      code: 'SERVICE_UNAVAILABLE',
    })
    expect(historyImageCacheSize()).toBe(51)
    releaseBlobImage('/thumbnail-0')
    await getBlobImageObjectUrl('/replacement')
    expect(historyImageCacheSize()).toBe(51)
    expect(peekBlobImageObjectUrl('/selected-detail')).toBe(selected)
    releaseSelected()
    expect(peekBlobImageObjectUrl('/selected-detail')).toBeNull()
    for (let i = 1; i < 50; i++) releaseBlobImage(`/thumbnail-${i}`)
  })

  it('keeps the normal budget at 50 when no selected slot is used', async () => {
    for (let i = 0; i < 70; i++) await getBlobImageObjectUrl(`/buffer-${i}`)
    expect(historyImageCacheSize()).toBe(50)
  })
  it("cancels the selected reservation's unfinished request after its final owner leaves", async () => {
    let finish!: (value: Blob) => void
    vi.mocked(daemonClient.fetchBlob).mockImplementationOnce(
      () =>
        new Promise(resolve => {
          finish = resolve
        })
    )
    const releaseSelected = reserveSelectedBlobImage('/pending-selected')
    const request = getBlobImageObjectUrl('/pending-selected')
    const signal = vi.mocked(daemonClient.fetchBlob).mock.calls.at(-1)![1]?.signal
    releaseSelected()
    expect(signal?.aborted).toBe(true)
    finish(new Blob(['late']))
    expect(await request).toBeNull()
    expect(historyImageCacheSize()).toBe(0)
  })
})
