import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardPreview } from '../useClipboardPreview'
import { clearClipboardPreviewCache } from '@/lib/clipboard-preview-cache'

vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntryResource: vi.fn(),
  getClipboardEntryDetail: vi.fn(),
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
  },
}))

// useClipboardPreview 内部嵌了 useEntryDelivery —— 它会订阅 Tauri 事件并
// 调 `getEntryDeliveryView` Tauri command。本测试只验证 preview 路径,
// 把 delivery 这条侧链全部桩掉,避免触达真实 Tauri runtime 产生
// unhandled rejection。
vi.mock('@/lib/ipc', () => ({
  events: {
    clipboardDeliveryStatusChanged: {
      listen: vi.fn(() => Promise.resolve(() => {})),
    },
  },
}))

vi.mock('@/api/tauri-command/clipboard_delivery', () => ({
  getEntryDeliveryView: vi.fn().mockResolvedValue(null),
}))

describe('useClipboardPreview', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clearClipboardPreviewCache()
  })

  it('starts empty when no preview target is selected', () => {
    const { result } = renderHook(() => useClipboardPreview(null))

    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(result.current.preview).toBeNull()
  })

  it('loads text preview content for text entries', async () => {
    const { getClipboardEntryResource, getClipboardEntryDetail } =
      await import('@/api/daemon/clipboard')

    vi.mocked(getClipboardEntryResource).mockResolvedValue({
      blobId: null,
      mimeType: 'text/plain',
      sizeBytes: 13,
      url: null,
      inlineData: btoa('Hello, World!'),
    })
    vi.mocked(getClipboardEntryDetail).mockResolvedValue({
      id: 'entry-1',
      content: 'Hello, World!',
      sizeBytes: 13,
      createdAtMs: 1710000000000,
      activeTimeMs: 1710000000000,
      mimeType: 'text/plain',
    })

    const { result, rerender } = renderHook(({ entryId }) => useClipboardPreview(entryId), {
      initialProps: { entryId: null as string | null },
    })

    rerender({ entryId: 'entry-1' })

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
      expect(result.current.preview).toMatchObject({
        entryId: 'entry-1',
        contentType: 'text',
        textContent: 'Hello, World!',
      })
    })
  })

  it('loads image preview content for image entries', async () => {
    const { getClipboardEntryResource } = await import('@/api/daemon/clipboard')

    vi.mocked(getClipboardEntryResource).mockResolvedValue({
      blobId: 'blob-123',
      mimeType: 'image/png',
      sizeBytes: 1024,
      url: '/clipboard/blobs/blob-123',
      inlineData: null,
    })

    const { result } = renderHook(() => useClipboardPreview('entry-2'))

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
      expect(result.current.preview).toMatchObject({
        entryId: 'entry-2',
        contentType: 'image',
        imageUrl: 'http://127.0.0.1:12345/clipboard/blobs/blob-123?auth=Session+test',
      })
    })
  })

  it('reuses cached preview data for the same entry within TTL', async () => {
    const { getClipboardEntryResource } = await import('@/api/daemon/clipboard')

    vi.mocked(getClipboardEntryResource).mockResolvedValue({
      blobId: 'blob-123',
      mimeType: 'image/png',
      sizeBytes: 1024,
      url: '/clipboard/blobs/blob-123',
      inlineData: null,
    })

    const first = renderHook(() => useClipboardPreview('entry-cache'))

    await waitFor(() => {
      expect(first.result.current.loading).toBe(false)
      expect(first.result.current.preview?.imageUrl).toContain('/clipboard/blobs/blob-123')
    })

    first.unmount()

    const second = renderHook(() => useClipboardPreview('entry-cache'))

    await waitFor(() => {
      expect(second.result.current.loading).toBe(false)
      expect(second.result.current.preview?.imageUrl).toContain('/clipboard/blobs/blob-123')
    })

    expect(getClipboardEntryResource).toHaveBeenCalledTimes(1)
  })
})
