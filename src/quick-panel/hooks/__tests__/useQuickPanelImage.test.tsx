import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  peekQuickPanelImageAspectRatio,
  reportQuickPanelImageAspectRatio,
  useQuickPanelImage,
  useQuickPanelImageAspectRatioEpoch,
} from '@/quick-panel/hooks/useQuickPanelImage'

const { getClipboardEntryResource } = vi.hoisted(() => ({
  getClipboardEntryResource: vi.fn(),
}))

vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntryResource,
}))

function inlineResource(data: string) {
  return {
    blobId: null,
    mimeType: 'image/png',
    sizeBytes: data.length,
    url: null,
    inlineData: data,
  }
}

describe('useQuickPanelImage', () => {
  beforeEach(() => {
    getClipboardEntryResource.mockReset()
    getClipboardEntryResource.mockResolvedValue(inlineResource('ZZZZ'))
  })

  it('resolves url via the shared descriptor + blob resolution', async () => {
    const { result } = renderHook(() => useQuickPanelImage('qp-entry-1'))
    await waitFor(() => expect(result.current.url).toBe('data:image/png;base64,ZZZZ'))
  })

  it('does not resolve url when disabled', async () => {
    const { result } = renderHook(() => useQuickPanelImage('qp-entry-disabled', { enabled: false }))
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current.url).toBeNull()
    expect(getClipboardEntryResource).not.toHaveBeenCalled()
  })

  it('picks up an aspect ratio already published by another instance', () => {
    reportQuickPanelImageAspectRatio('qp-entry-preseeded', 1.5)
    const { result } = renderHook(() =>
      useQuickPanelImage('qp-entry-preseeded', { enabled: false })
    )
    expect(result.current.aspectRatio).toBe(1.5)
  })

  it('re-renders when another writer publishes a new aspect ratio for the same entry', () => {
    const { result } = renderHook(() =>
      useQuickPanelImage('qp-entry-subscribe', { enabled: false })
    )
    expect(result.current.aspectRatio).toBeUndefined()

    act(() => {
      reportQuickPanelImageAspectRatio('qp-entry-subscribe', 0.75)
    })
    expect(result.current.aspectRatio).toBe(0.75)
  })

  it('resets aspect ratio synchronously when entryId changes (no stale flash)', () => {
    reportQuickPanelImageAspectRatio('qp-entry-switch-a', 2)
    const { result, rerender } = renderHook(
      ({ entryId }: { entryId: string }) => useQuickPanelImage(entryId, { enabled: false }),
      { initialProps: { entryId: 'qp-entry-switch-a' } }
    )
    expect(result.current.aspectRatio).toBe(2)

    rerender({ entryId: 'qp-entry-switch-b' })
    // No aspect ratio published yet for -b — must not still show -a's ratio.
    expect(result.current.aspectRatio).toBeUndefined()
  })

  it('reportQuickPanelImageAspectRatio is a no-op for non-finite or non-positive values', () => {
    reportQuickPanelImageAspectRatio('qp-entry-invalid', Number.NaN)
    reportQuickPanelImageAspectRatio('qp-entry-invalid', 0)
    reportQuickPanelImageAspectRatio('qp-entry-invalid', -1)
    expect(peekQuickPanelImageAspectRatio('qp-entry-invalid')).toBeUndefined()
  })

  it('useQuickPanelImageAspectRatioEpoch increments whenever any entry publishes a new ratio', () => {
    const { result } = renderHook(() => useQuickPanelImageAspectRatioEpoch())
    const initial = result.current

    act(() => {
      reportQuickPanelImageAspectRatio('qp-entry-epoch', 1.2)
    })
    expect(result.current).toBe(initial + 1)
  })
})
