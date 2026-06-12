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

  it('coalesces rapid local items into a single trailing list fetch', async () => {
    const { getClipboardEntries } = await import('@/api/daemon/clipboard')
    const mockGetClipboardEntries = vi.mocked(getClipboardEntries)
    const onLocalItem = vi.fn()

    const mkEntry = (id: string) => ({
      id,
      preview: id,
      hasDetail: false,
      sizeBytes: 1,
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
    })
    mockGetClipboardEntries.mockResolvedValue({
      status: 'ready',
      entries: [mkEntry('e1'), mkEntry('e2'), mkEntry('e3')],
    })

    renderHook(() =>
      useClipboardEventStream({
        onLocalItem,
        onRemoteInvalidate: vi.fn(),
        onDeleted: vi.fn(),
      })
    )

    await waitFor(() => expect(capturedHandler).not.toBeNull())
    vi.useFakeTimers()

    const fireLocal = (id: string) =>
      capturedHandler?.({
        topic: 'clipboard',
        eventType: 'clipboard.new_content',
        ts: 0,
        sessionId: null,
        payload: { entryId: id, preview: id, origin: 'local' },
      })

    // First copy: leading-edge fetch fires immediately.
    await act(async () => {
      fireLocal('e1')
      await Promise.resolve()
    })
    expect(mockGetClipboardEntries).toHaveBeenCalledTimes(1)

    // Two more copies within the throttle window: coalesced, no extra fetch yet.
    act(() => {
      fireLocal('e2')
      fireLocal('e3')
    })
    expect(mockGetClipboardEntries).toHaveBeenCalledTimes(1)

    // After the window, a single trailing fetch covers both pending ids.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300)
    })
    expect(mockGetClipboardEntries).toHaveBeenCalledTimes(2)

    // Every entry id was delivered to onLocalItem exactly once, none dropped.
    expect(onLocalItem).toHaveBeenCalledWith(expect.objectContaining({ id: 'e1' }))
    expect(onLocalItem).toHaveBeenCalledWith(expect.objectContaining({ id: 'e2' }))
    expect(onLocalItem).toHaveBeenCalledWith(expect.objectContaining({ id: 'e3' }))
    expect(onLocalItem).toHaveBeenCalledTimes(3)
    vi.useRealTimers()
  })

  // Note: clipboard.deleted is never emitted by the daemon — test omitted.
})
