import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardPreview } from '../useClipboardPreview'

vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntryResource: vi.fn(),
  getClipboardEntryDetail: vi.fn(),
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
  },
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('useClipboardPreview', () => {
  beforeEach(() => {
    vi.clearAllMocks()
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
      inlineData: null,
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

  it('ignores stale responses when the selected entry changes', async () => {
    const { getClipboardEntryResource, getClipboardEntryDetail } =
      await import('@/api/daemon/clipboard')

    const firstResource = deferred<{
      blobId: string | null
      mimeType: string
      sizeBytes: number
      url: string | null
      inlineData: string | null
    }>()
    const secondResource = deferred<{
      blobId: string | null
      mimeType: string
      sizeBytes: number
      url: string | null
      inlineData: string | null
    }>()

    vi.mocked(getClipboardEntryResource)
      .mockReturnValueOnce(firstResource.promise)
      .mockReturnValueOnce(secondResource.promise)
    vi.mocked(getClipboardEntryDetail).mockImplementation(async id => {
      if (id === 'entry-1') {
        return {
          id: 'entry-1',
          content: 'Old preview',
          sizeBytes: 11,
          createdAtMs: 1710000000000,
          activeTimeMs: 1710000000000,
          mimeType: 'text/plain',
        }
      }

      return {
        id: 'entry-2',
        content: 'Fresh preview',
        sizeBytes: 13,
        createdAtMs: 1710000000000,
        activeTimeMs: 1710000000000,
        mimeType: 'text/plain',
      }
    })

    const { result, rerender } = renderHook(({ entryId }) => useClipboardPreview(entryId), {
      initialProps: { entryId: 'entry-1' },
    })

    rerender({ entryId: 'entry-2' })

    secondResource.resolve({
      blobId: null,
      mimeType: 'text/plain',
      sizeBytes: 13,
      url: null,
      inlineData: null,
    })

    await waitFor(() => {
      expect(result.current.preview).toMatchObject({
        entryId: 'entry-2',
        textContent: 'Fresh preview',
      })
    })

    firstResource.resolve({
      blobId: null,
      mimeType: 'text/plain',
      sizeBytes: 11,
      url: null,
      inlineData: null,
    })

    await waitFor(() => {
      expect(result.current.preview).toMatchObject({
        entryId: 'entry-2',
        textContent: 'Fresh preview',
      })
    })
  })
})
