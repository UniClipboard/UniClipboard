import { listen } from '@tauri-apps/api/event'
import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardEventStream } from '../useClipboardEventStream'
import { getClipboardEntry } from '@/api/clipboardItems'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

vi.mock('@/api/clipboardItems', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/clipboardItems')>()
  return {
    ...actual,
    getClipboardEntry: vi.fn(),
  }
})

const mockListen = vi.mocked(listen)
const mockGetClipboardEntry = vi.mocked(getClipboardEntry)

describe('useClipboardEventStream', () => {
  let callback:
    | ((event: { payload: { type: string; entry_id?: string; origin?: string } }) => void)
    | null = null

  beforeEach(() => {
    vi.clearAllMocks()
    callback = null
    mockListen.mockImplementation(async (_channel: string, cb: unknown) => {
      callback = cb as typeof callback
      return (() => {}) as () => void
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('loads single local item and emits onLocalItem', async () => {
    const onLocalItem = vi.fn()
    mockGetClipboardEntry.mockResolvedValue({
      id: 'entry-1',
      is_downloaded: true,
      is_favorited: false,
      created_at: 0,
      updated_at: 0,
      active_time: 0,
      item: { text: { display_text: 'hello', has_detail: false, size: 5 } },
    } as never)

    renderHook(() =>
      useClipboardEventStream({
        onLocalItem,
        onRemoteInvalidate: vi.fn(),
        onDeleted: vi.fn(),
      })
    )

    await waitFor(() => expect(callback).not.toBeNull())

    await act(async () => {
      callback?.({ payload: { type: 'NewContent', entry_id: 'entry-1', origin: 'local' } })
      await Promise.resolve()
    })

    expect(mockGetClipboardEntry).toHaveBeenCalledWith('entry-1')
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

    await waitFor(() => expect(callback).not.toBeNull())
    vi.useFakeTimers()

    act(() => {
      callback?.({ payload: { type: 'NewContent', entry_id: 'entry-1', origin: 'remote' } })
      callback?.({ payload: { type: 'NewContent', entry_id: 'entry-2', origin: 'remote' } })
    })

    expect(onRemoteInvalidate).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(300)
    })

    expect(onRemoteInvalidate).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  it('forwards delete events', async () => {
    const onDeleted = vi.fn()

    renderHook(() =>
      useClipboardEventStream({
        onLocalItem: vi.fn(),
        onRemoteInvalidate: vi.fn(),
        onDeleted,
      })
    )

    await waitFor(() => expect(callback).not.toBeNull())

    act(() => {
      callback?.({ payload: { type: 'Deleted', entry_id: 'entry-9' } })
    })

    expect(onDeleted).toHaveBeenCalledWith('entry-9')
  })
})
