import { describe, expect, it, vi } from 'vitest'
import {
  getClipboardStats,
  favoriteClipboardItem,
  unfavoriteClipboardItem,
  resolveResourceImageUrl,
} from '@/api/clipboardItems'

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
    request: vi.fn(),
  },
}))

const mockDaemonClipboard = vi.hoisted(() => ({
  deleteClipboardEntry: vi.fn(),
  restoreClipboardEntry: vi.fn(),
  toggleFavorite: vi.fn(),
  clearClipboardHistory: vi.fn(),
  getClipboardStats: vi.fn(),
  getClipboardEntryResource: vi.fn(),
  getEntryDetail: vi.fn(),
}))

vi.mock('@/api/daemon/clipboard', () => mockDaemonClipboard)

describe('getClipboardStats', () => {
  it('returns stats from daemon', async () => {
    mockDaemonClipboard.getClipboardStats.mockResolvedValueOnce({
      totalItems: 3,
      totalSize: 1024,
    })

    const result = await getClipboardStats()

    expect(result).toEqual({ total_items: 3, total_size: 1024 })
  })
})

describe('favoriteClipboardItem / unfavoriteClipboardItem', () => {
  it('calls toggleFavorite with true when favoriting', async () => {
    mockDaemonClipboard.toggleFavorite.mockResolvedValueOnce(undefined)

    await favoriteClipboardItem('entry-1')

    expect(mockDaemonClipboard.toggleFavorite).toHaveBeenCalledWith('entry-1', true)
  })

  it('calls toggleFavorite with false when unfavoriting', async () => {
    mockDaemonClipboard.toggleFavorite.mockResolvedValueOnce(undefined)

    await unfavoriteClipboardItem('entry-1')

    expect(mockDaemonClipboard.toggleFavorite).toHaveBeenCalledWith('entry-1', false)
  })
})

describe('resolveResourceImageUrl', () => {
  it('keeps inline data URLs unchanged', () => {
    const resource = {
      blobId: null,
      mimeType: 'image/png',
      sizeBytes: 4,
      url: null,
      inlineData: 'iVBORw0KGgo=',
    }

    expect(resolveResourceImageUrl(resource)).toBe('data:image/png;base64,iVBORw0KGgo=')
  })

  it('upgrades daemon blob paths to authenticated daemon URLs', () => {
    const resource = {
      blobId: 'blob-1',
      mimeType: 'image/png',
      sizeBytes: 123,
      url: '/clipboard/blobs/blob-1',
      inlineData: null,
    }

    expect(resolveResourceImageUrl(resource)).toBe(
      'http://127.0.0.1:12345/clipboard/blobs/blob-1?auth=Session+test'
    )
  })
})
