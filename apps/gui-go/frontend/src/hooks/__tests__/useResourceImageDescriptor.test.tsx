import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  invalidateResourceImageDescriptor,
  useResourceImageDescriptor,
} from '@/hooks/useResourceImageDescriptor'

const { getClipboardEntryResource } = vi.hoisted(() => ({
  getClipboardEntryResource: vi.fn(),
}))

vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntryResource,
}))

function inlineResource(mimeType: string, data: string) {
  return { blobId: null, mimeType, sizeBytes: data.length, url: null, inlineData: data }
}

function blobResource(url: string) {
  return { blobId: 'blob-1', mimeType: 'image/png', sizeBytes: 100, url, inlineData: null }
}

describe('useResourceImageDescriptor', () => {
  beforeEach(() => {
    getClipboardEntryResource.mockReset()
  })

  it('resolves an inline resource to a data: URL', async () => {
    getClipboardEntryResource.mockResolvedValue(inlineResource('image/png', 'AAAA'))
    const { result } = renderHook(() => useResourceImageDescriptor('entry-inline'))
    expect(result.current).toBeNull()
    await waitFor(() => expect(result.current).toBe('data:image/png;base64,AAAA'))
  })

  it('resolves a blob-backed resource to its daemon path', async () => {
    getClipboardEntryResource.mockResolvedValue(blobResource('/clipboard/blobs/abc'))
    const { result } = renderHook(() => useResourceImageDescriptor('entry-blob'))
    await waitFor(() => expect(result.current).toBe('/clipboard/blobs/abc'))
  })

  it('caches per entry id — a second mount does not re-fetch', async () => {
    getClipboardEntryResource.mockResolvedValue(inlineResource('image/png', 'BBBB'))
    const first = renderHook(() => useResourceImageDescriptor('entry-cache-hit'))
    await waitFor(() => expect(first.result.current).not.toBeNull())
    expect(getClipboardEntryResource).toHaveBeenCalledTimes(1)

    const second = renderHook(() => useResourceImageDescriptor('entry-cache-hit'))
    await waitFor(() => expect(second.result.current).toBe('data:image/png;base64,BBBB'))
    expect(getClipboardEntryResource).toHaveBeenCalledTimes(1)
  })

  it('dedupes concurrent cold-cache lookups for the same entry id', async () => {
    let resolveFetch: (value: unknown) => void = () => {}
    getClipboardEntryResource.mockImplementation(
      () =>
        new Promise(resolve => {
          resolveFetch = resolve
        })
    )
    const first = renderHook(() => useResourceImageDescriptor('entry-dedup'))
    const second = renderHook(() => useResourceImageDescriptor('entry-dedup'))
    expect(getClipboardEntryResource).toHaveBeenCalledTimes(1)

    await act(async () => {
      resolveFetch(inlineResource('image/png', 'CCCC'))
      await Promise.resolve()
    })
    await waitFor(() => expect(first.result.current).toBe('data:image/png;base64,CCCC'))
    await waitFor(() => expect(second.result.current).toBe('data:image/png;base64,CCCC'))
  })

  it('does not fetch when disabled', async () => {
    getClipboardEntryResource.mockResolvedValue(inlineResource('image/png', 'DDDD'))
    const { result } = renderHook(() => useResourceImageDescriptor('entry-disabled', false))
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current).toBeNull()
    expect(getClipboardEntryResource).not.toHaveBeenCalled()
  })

  it('resets synchronously (no stale flash) when entryId changes', async () => {
    getClipboardEntryResource.mockImplementation((id: string) =>
      Promise.resolve(inlineResource('image/png', id))
    )
    const { result, rerender } = renderHook(
      ({ entryId }: { entryId: string }) => useResourceImageDescriptor(entryId),
      { initialProps: { entryId: 'entry-switch-a' } }
    )
    await waitFor(() => expect(result.current).toBe('data:image/png;base64,entry-switch-a'))

    rerender({ entryId: 'entry-switch-b' })
    // Synchronous reset: even before the new fetch resolves, the old
    // descriptor must not still be showing.
    expect(result.current).toBeNull()

    await waitFor(() => expect(result.current).toBe('data:image/png;base64,entry-switch-b'))
  })

  it('does not cache a failed lookup, so a later render can retry', async () => {
    getClipboardEntryResource.mockRejectedValueOnce(new Error('boom'))
    const { result, unmount } = renderHook(() => useResourceImageDescriptor('entry-fail'))
    await waitFor(() => expect(result.current).toBeNull())
    unmount()

    getClipboardEntryResource.mockResolvedValueOnce(inlineResource('image/png', 'EEEE'))
    const { result: retried } = renderHook(() => useResourceImageDescriptor('entry-fail'))
    await waitFor(() => expect(retried.current).toBe('data:image/png;base64,EEEE'))
    expect(getClipboardEntryResource).toHaveBeenCalledTimes(2)
  })

  it('invalidateResourceImageDescriptor drops the cache so the next lookup re-fetches', async () => {
    getClipboardEntryResource.mockResolvedValue(inlineResource('image/png', 'FFFF'))
    const { result, unmount } = renderHook(() => useResourceImageDescriptor('entry-invalidate'))
    await waitFor(() => expect(result.current).not.toBeNull())
    expect(getClipboardEntryResource).toHaveBeenCalledTimes(1)
    unmount()

    invalidateResourceImageDescriptor('entry-invalidate')
    renderHook(() => useResourceImageDescriptor('entry-invalidate'))
    await waitFor(() => expect(getClipboardEntryResource).toHaveBeenCalledTimes(2))
  })
})
