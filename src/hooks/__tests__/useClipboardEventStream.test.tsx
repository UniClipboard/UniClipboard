import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardEventStream } from '../useClipboardEventStream'

// Mock daemonWs (hook now uses daemonWs.subscribe)
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((_topics, handler) => {
      // Capture the handler so tests can invoke it directly.
      capturedHandler = handler
      return () => {
        capturedHandler = null
      }
    }),
  },
}))

// Mock clipboardItems (isImageType) — the hook imports isImageType from here
vi.mock('@/api/clipboardItems', () => ({
  isImageType: vi.fn(() => false),
}))

// Mock daemon clipboard module (getClipboardEntries) — the hook imports from here
vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntries: vi.fn(),
}))

// The hook now dispatches to redux for incoming-pending placeholder rows;
// these tests don't exercise that path, so a no-op dispatch keeps the
// surface area small without pulling in a full store + Provider.
vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => vi.fn(),
}))

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let capturedHandler: ((event: any) => void) | null = null

describe('useClipboardEventStream', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    capturedHandler = null
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('loads single local item and emits onLocalItem', async () => {
    const { getClipboardEntries } = await import('@/api/daemon/clipboard')
    const mockGetClipboardEntries = vi.mocked(getClipboardEntries)
    const onLocalItem = vi.fn()

    mockGetClipboardEntries.mockResolvedValue({
      status: 'ready',
      entries: [
        {
          id: 'entry-1',
          preview: 'hello',
          hasDetail: false,
          sizeBytes: 5,
          capturedAt: 0,
          contentType: 'text/plain',
          thumbnailUrl: null,
          isEncrypted: false,
          isFavorited: false,
          updatedAt: 0,
          activeTime: 0,
          fileTransferStatus: null,
          fileTransferReason: null,
          linkUrls: null,
          linkDomains: null,
          fileSizes: null,
        },
      ],
    })

    renderHook(() =>
      useClipboardEventStream({
        onLocalItem,
        onRemoteInvalidate: vi.fn(),
        onDeleted: vi.fn(),
      })
    )

    await waitFor(() => expect(capturedHandler).not.toBeNull())

    await act(async () => {
      capturedHandler?.({
        topic: 'clipboard',
        eventType: 'clipboard.new_content',
        ts: 0,
        sessionId: null,
        payload: { entryId: 'entry-1', preview: 'hello', origin: 'local' },
      })
      await Promise.resolve()
    })

    expect(mockGetClipboardEntries).toHaveBeenCalled()
    expect(onLocalItem).toHaveBeenCalledWith(expect.objectContaining({ id: 'entry-1' }))
  })

  it('throttles remote invalidation', async () => {
    const onRemoteInvalidate = vi.fn()

    renderHook(() =>
      useClipboardEventStream({
        onLocalItem: vi.fn(),
        onRemoteInvalidate,
        onDeleted: vi.fn(),
      })
    )

    await waitFor(() => expect(capturedHandler).not.toBeNull())
    vi.useFakeTimers()

    act(() => {
      capturedHandler?.({
        topic: 'clipboard',
        eventType: 'clipboard.new_content',
        ts: 0,
        sessionId: null,
        payload: { entryId: 'entry-1', preview: '...', origin: 'remote' },
      })
      capturedHandler?.({
        topic: 'clipboard',
        eventType: 'clipboard.new_content',
        ts: 0,
        sessionId: null,
        payload: { entryId: 'entry-2', preview: '...', origin: 'remote' },
      })
    })

    expect(onRemoteInvalidate).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(300)
    })

    expect(onRemoteInvalidate).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  // Note: clipboard.deleted is never emitted by the daemon — test omitted.
})
