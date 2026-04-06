import { describe, expect, it, vi } from 'vitest'
import {
  getClipboardItems,
  getClipboardStats,
  favoriteClipboardItem,
  unfavoriteClipboardItem,
  getClipboardItem,
  resolveResourceImageUrl,
} from '@/api/clipboardItems'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
  },
}))

const invokeMock = invokeWithTrace as unknown as ReturnType<typeof vi.fn>

describe('getClipboardItems', () => {
  it('将 image/* 条目映射为 image 类型，并优先使用后端返回的 thumbnail_url', async () => {
    invokeMock.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'entry-1',
          preview: 'Image (123 bytes)',
          has_detail: true,
          size_bytes: 123,
          captured_at: 1,
          content_type: 'image/png',
          is_encrypted: false,
          is_favorited: false,
          updated_at: 1,
          active_time: 1,
          thumbnail_url: 'uc://thumbnail/rep-1',
        },
      ],
    })

    const result = (await getClipboardItems()) as unknown as {
      status: string
      items?: Array<{ id: string; item: { text?: unknown; image?: { thumbnail?: string } } }>
    }

    expect(result.items).toHaveLength(1)
    expect(result.items?.[0].item.image).toBeTruthy()
    expect(result.items?.[0].item.text).toBeFalsy()
    expect(result.items?.[0].item.image?.thumbnail).toBe('uc://thumbnail/rep-1')
  })

  it('returns not_ready when backend is not ready', async () => {
    invokeMock.mockResolvedValue({ status: 'not_ready' })

    const result = (await getClipboardItems()) as unknown as { status: string }

    expect(result).toEqual({ status: 'not_ready' })
  })

  it('maps backend projections when ready', async () => {
    invokeMock.mockResolvedValue({
      status: 'ready',
      entries: [
        {
          id: 'entry-1',
          preview: 'hello',
          has_detail: true,
          size_bytes: 12,
          captured_at: 100,
          content_type: 'text/plain',
          is_encrypted: true,
          is_favorited: false,
          updated_at: 120,
          active_time: 130,
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
  it('calls get_clipboard_stats and returns stats', async () => {
    invokeMock.mockResolvedValueOnce({ total_items: 3, total_size: 1024 })

    const result = await getClipboardStats()

    expect(invokeMock).toHaveBeenCalledWith('get_clipboard_stats')
    expect(result).toEqual({ total_items: 3, total_size: 1024 })
  })
})

describe('favoriteClipboardItem / unfavoriteClipboardItem', () => {
  it('calls toggle_favorite_clipboard_item with is_favorited true when favoriting', async () => {
    invokeMock.mockResolvedValueOnce(undefined)

    await favoriteClipboardItem('entry-1')

    expect(invokeMock).toHaveBeenCalledWith('toggle_favorite_clipboard_item', {
      id: 'entry-1',
      is_favorited: true,
    })
  })

  it('calls toggle_favorite_clipboard_item with is_favorited false when unfavoriting', async () => {
    invokeMock.mockResolvedValueOnce(undefined)

    await unfavoriteClipboardItem('entry-1')

    expect(invokeMock).toHaveBeenCalledWith('toggle_favorite_clipboard_item', {
      id: 'entry-1',
      is_favorited: false,
    })
  })
})

describe('file transfer status hydration', () => {
  it('hydrates failed file_transfer_status from API response', async () => {
    invokeMock.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'file-entry-1',
          preview: 'file:///tmp/test.txt',
          has_detail: false,
          size_bytes: 100,
          captured_at: 1000,
          content_type: 'text/uri-list',
          is_encrypted: false,
          is_favorited: false,
          updated_at: 1000,
          active_time: 0,
          file_transfer_status: 'failed',
          file_transfer_reason: 'timeout after 60s',
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

  it('hydrates pending file_transfer_status from API response', async () => {
    invokeMock.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'file-entry-2',
          preview: 'file:///tmp/doc.pdf',
          has_detail: false,
          size_bytes: 5000,
          captured_at: 2000,
          content_type: 'text/uri-list',
          is_encrypted: false,
          is_favorited: false,
          updated_at: 2000,
          active_time: 0,
          file_transfer_status: 'pending',
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

    expect(result.items[0].file_transfer_status).toBe('pending')
    expect(result.items[0].file_transfer_reason).toBeNull()
  })

  it('returns null file_transfer_status for non-file entries', async () => {
    invokeMock.mockResolvedValueOnce({
      status: 'ready',
      entries: [
        {
          id: 'text-entry-1',
          preview: 'hello world',
          has_detail: false,
          size_bytes: 11,
          captured_at: 3000,
          content_type: 'text/plain',
          is_encrypted: false,
          is_favorited: false,
          updated_at: 3000,
          active_time: 0,
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

describe('getClipboardItem', () => {
  it('calls get_clipboard_item with id and fullContent', async () => {
    const response = {
      id: 'entry-1',
      is_downloaded: true,
      is_favorited: false,
      created_at: 0,
      updated_at: 0,
      active_time: 0,
      item: {
        text: null,
        image: null,
        file: null,
        link: null,
        code: null,
        unknown: null,
      },
    }

    invokeMock.mockResolvedValueOnce(response)

    const result = await getClipboardItem('entry-1', true)

    expect(invokeMock).toHaveBeenCalledWith('get_clipboard_item', {
      id: 'entry-1',
      fullContent: true,
    })
    expect(result).toEqual(response)
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
