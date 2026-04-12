import { describe, expect, it, vi } from 'vitest'
import {
  getClipboardItems,
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

// Mock the daemon clipboard module
const mockDaemonClipboard = vi.hoisted(() => ({
  getClipboardEntries: vi.fn(),
  getClipboardEntry: vi.fn(),
  deleteClipboardEntry: vi.fn(),
  restoreClipboardEntry: vi.fn(),
  toggleFavorite: vi.fn(),
  clearClipboardHistory: vi.fn(),
  getClipboardStats: vi.fn(),
  getClipboardEntryResource: vi.fn(),
  getEntryDetail: vi.fn(),
}))

vi.mock('@/api/daemon/clipboard', () => mockDaemonClipboard)

describe('getClipboardItems', () => {
  it('将 image/* 条目映射为 image 类型，并优先使用后端返回的 thumbnailUrl', async () => {
    mockDaemonClipboard.getClipboardEntries.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'entry-1',
          preview: 'Image (123 bytes)',
          hasDetail: true,
          sizeBytes: 123,
          capturedAt: 1,
          contentType: 'image/png',
          isEncrypted: false,
          isFavorited: false,
          updatedAt: 1,
          activeTime: 1,
          thumbnailUrl: 'uc://thumbnail/rep-1',
          fileTransferStatus: null,
          fileTransferReason: null,
          linkUrls: null,
          linkDomains: null,
          fileSizes: null,
          imageWidth: 1920,
          imageHeight: 1080,
        },
      ],
    })

    const result = (await getClipboardItems()) as unknown as {
      status: string
      items?: Array<{
        id: string
        item: { text?: unknown; image?: { thumbnail?: string; width: number; height: number } }
      }>
    }

    expect(result.items).toHaveLength(1)
    expect(result.items?.[0].item.image).toBeTruthy()
    expect(result.items?.[0].item.text).toBeFalsy()
    expect(result.items?.[0].item.image?.thumbnail).toBe('uc://thumbnail/rep-1')
    expect(result.items?.[0].item.image?.width).toBe(1920)
    expect(result.items?.[0].item.image?.height).toBe(1080)
  })

  it('returns not_ready when backend is not ready', async () => {
    mockDaemonClipboard.getClipboardEntries.mockResolvedValueOnce({ status: 'not_ready' })

    const result = (await getClipboardItems()) as unknown as { status: string }

    expect(result).toEqual({ status: 'not_ready' })
  })

  it('maps backend projections when ready', async () => {
    mockDaemonClipboard.getClipboardEntries.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'entry-1',
          preview: 'hello',
          hasDetail: true,
          sizeBytes: 12,
          capturedAt: 100,
          contentType: 'text/plain',
          isEncrypted: true,
          isFavorited: false,
          updatedAt: 120,
          activeTime: 130,
          thumbnailUrl: null,
          fileTransferStatus: null,
          fileTransferReason: null,
          linkUrls: null,
          linkDomains: null,
          fileSizes: null,
        },
      ],
    })

    const result = (await getClipboardItems()) as unknown as {
      status: string
      items?: Array<{ id: string; item: { text: { display_text: string } } }>
    }

    expect(result.status).toBe('ready')
    expect(result.items?.[0].id).toBe('entry-1')
    expect(result.items?.[0].item.text.display_text).toBe('hello')
  })
})

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

describe('file transfer status hydration', () => {
  it('hydrates failed file_transfer_status from API response', async () => {
    mockDaemonClipboard.getClipboardEntries.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'file-entry-1',
          preview: 'file:///tmp/test.txt',
          hasDetail: false,
          sizeBytes: 100,
          capturedAt: 1000,
          contentType: 'text/uri-list',
          isEncrypted: false,
          isFavorited: false,
          updatedAt: 1000,
          activeTime: 0,
          thumbnailUrl: null,
          fileTransferStatus: 'failed',
          fileTransferReason: 'timeout after 60s',
          linkUrls: null,
          linkDomains: null,
          fileSizes: null,
        },
      ],
    })

    const result = (await getClipboardItems()) as {
      status: string
      items: Array<{
        id: string
        file_transfer_status?: string | null
        file_transfer_reason?: string | null
      }>
    }

    expect(result.status).toBe('ready')
    expect(result.items[0].file_transfer_status).toBe('failed')
    expect(result.items[0].file_transfer_reason).toBe('timeout after 60s')
  })

  it('returns null file_transfer_status for non-file entries', async () => {
    mockDaemonClipboard.getClipboardEntries.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'text-entry-1',
          preview: 'hello world',
          hasDetail: false,
          sizeBytes: 11,
          capturedAt: 3000,
          contentType: 'text/plain',
          isEncrypted: false,
          isFavorited: false,
          updatedAt: 3000,
          activeTime: 0,
          thumbnailUrl: null,
          fileTransferStatus: null,
          fileTransferReason: null,
          linkUrls: null,
          linkDomains: null,
          fileSizes: null,
        },
      ],
    })

    const result = (await getClipboardItems()) as {
      status: string
      items: Array<{ id: string; file_transfer_status?: string | null }>
    }

    expect(result.items[0].file_transfer_status).toBeNull()
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
