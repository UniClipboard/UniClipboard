import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { querySearch } from '@/api/daemon/search'
import type { LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import { useHistoryWindow } from '@/hooks/useHistoryWindow'

vi.mock('@/api/daemon/search', () => ({ querySearch: vi.fn() }))
vi.mock('@/hooks/useEncryptionSessionState', () => ({
  useEncryptionSessionState: () => ({ encryptionReady: true, isLocked: false }),
}))
vi.mock('@/hooks/useClipboardEventStream', () => ({ useClipboardEventStream: vi.fn() }))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: () => () => {},
    onReconnect: () => () => {},
  },
}))
vi.mock('@/lib/clipboard-transform', () => ({
  searchResultToDisplayItem: (item: unknown) => item,
}))

const model: LiveSearchQueryModel = { query: '', timeRange: 'all_time' }
function page(offset: number, limit: number, total = 200, prefix = '') {
  return {
    data: {
      items: Array.from({ length: Math.max(0, Math.min(limit, total - offset)) }, (_, i) => ({
        id: `${prefix}${offset + i}`,
        type: 'text',
        activeTime: offset + i,
        content: null,
      })),
      total,
      state: 'ready',
      hasMore: offset + limit < total,
    },
    ts: 0,
  } as unknown as Awaited<ReturnType<typeof querySearch>>
}
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(r => {
    resolve = r
  })
  return { promise, resolve }
}

describe('useHistoryWindow', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.mocked(querySearch)
      .mockReset()
      .mockImplementation(async params => page(params.offset ?? 0, params.limit ?? 50))
  })
  afterEach(() => vi.useRealTimers())

  it('publishes the full replacement atomically and exposes total, never more than 50 records', async () => {
    const published: string[][] = []
    const { result } = renderHook(() => {
      const window = useHistoryWindow(model)
      published.push(window.items.map(item => item.id))
      return window
    })
    await act(async () => {})
    expect(result.current.total).toBe(200)
    const pending = deferred<Awaited<ReturnType<typeof querySearch>>>()
    vi.mocked(querySearch).mockReturnValueOnce(pending.promise)
    act(() => result.current.reportViewport(35, 44, true))
    expect(result.current.offset).toBe(0)
    expect(result.current.items).toHaveLength(50)
    await act(async () => pending.resolve(page(50, 25)))
    expect(result.current.offset).toBe(25)
    expect(result.current.items.map(item => item.id)).toEqual(
      Array.from({ length: 50 }, (_, i) => String(25 + i))
    )
    expect(published.every(items => items.length === 0 || items.length === 50)).toBe(true)
  })

  it('aborts superseded work and ignores its late response', async () => {
    const { result, rerender } = renderHook(({ model }) => useHistoryWindow(model), {
      initialProps: { model },
    })
    await act(async () => {})
    const stale = deferred<Awaited<ReturnType<typeof querySearch>>>()
    vi.mocked(querySearch).mockReturnValueOnce(stale.promise)
    act(() => result.current.reportViewport(35, 44, true))
    const signal = vi.mocked(querySearch).mock.calls.at(-1)![1]!
    rerender({ model: { query: 'new' } })
    await act(async () => {})
    expect(signal.aborted).toBe(true)
    await act(async () => stale.resolve(page(50, 25, 999, 'stale')))
    expect(result.current.total).toBe(200)
    expect(result.current.items.some(item => item.id.startsWith('stale'))).toBe(false)
  })

  it('shrinks to the visible range with ten buffered records after 750 ms', async () => {
    const { result } = renderHook(() => useHistoryWindow(model))
    await act(async () => {})
    act(() => result.current.reportViewport(10, 14, true))
    await act(async () => {
      vi.advanceTimersByTime(749)
    })
    expect(result.current.items).toHaveLength(50)
    await act(async () => {
      vi.advanceTimersByTime(1)
    })
    expect(result.current.offset).toBe(0)
    expect(result.current.items).toHaveLength(25)
    expect(result.current.total).toBe(200)
  })
  it('supersedes fast consecutive scrolls and inherits authoritative refresh ownership through idle shrink', async () => {
    const { result } = renderHook(() => useHistoryWindow(model))
    await act(async () => {})
    const refresh = deferred<Awaited<ReturnType<typeof querySearch>>>()
    const superseded = deferred<Awaited<ReturnType<typeof querySearch>>>()
    vi.mocked(querySearch)
      .mockReturnValueOnce(refresh.promise)
      .mockReturnValueOnce(superseded.promise)
    act(() => result.current.refetch())
    const refreshSignal = vi.mocked(querySearch).mock.calls.at(-1)![1]!
    act(() => result.current.reportViewport(80, 84, true))
    const movingSignal = vi.mocked(querySearch).mock.calls.at(-1)![1]!
    expect(refreshSignal.aborted).toBe(true)
    expect(vi.mocked(querySearch).mock.calls.at(-1)![0]).toMatchObject({ offset: 70, limit: 50 })
    await act(async () => {
      vi.advanceTimersByTime(750)
    })
    expect(movingSignal.aborted).toBe(true)
    expect(vi.mocked(querySearch).mock.calls.at(-1)![0]).toMatchObject({ offset: 70, limit: 25 })
    await act(async () => {
      refresh.resolve(page(0, 50, 200, 'stale'))
      superseded.resolve(page(70, 50, 200, 'stale'))
    })
    expect(result.current.items).toHaveLength(25)
    expect(result.current.items.map(item => item.id)).toEqual(
      Array.from({ length: 25 }, (_, i) => String(70 + i))
    )
  })
  it('cancels a pending distant window when the user returns to the still-rendered cache', async () => {
    const { result } = renderHook(() => useHistoryWindow(model))
    await act(async () => {})
    const distant = deferred<Awaited<ReturnType<typeof querySearch>>>()
    vi.mocked(querySearch).mockReturnValueOnce(distant.promise)
    act(() => result.current.reportViewport(120, 124, true))
    const signal = vi.mocked(querySearch).mock.calls.at(-1)![1]!
    await act(async () => result.current.reportViewport(0, 4, true))
    expect(signal.aborted).toBe(true)
    await act(async () => distant.resolve(page(110, 50)))
    expect(result.current.offset).toBe(0)
    expect(result.current.items[0].id).toBe('0')
  })
  it('locates an evicted selection again when reordering invalidates its old absolute index', async () => {
    const { result } = renderHook(() => useHistoryWindow(model))
    await act(async () => {})
    await act(async () => result.current.reportViewport(120, 124, true))
    const reordered = Array.from({ length: 200 }, (_, i) => String(i)).filter(id => id !== '3')
    reordered.splice(60, 0, '3')
    vi.mocked(querySearch).mockImplementation(async params => {
      const response = page(params.offset ?? 0, params.limit ?? 50)
      response.data.items.forEach((item, i) => {
        Object.assign(item, { id: reordered[(params.offset ?? 0) + i] })
      })
      return response
    })
    let neighbor: Awaited<ReturnType<typeof result.current.ensureNeighbor>>
    await act(async () => {
      neighbor = await result.current.ensureNeighbor('3', 3, 1)
    })
    expect(neighbor!).toMatchObject({ item: { id: '61' }, index: 61 })
    expect(result.current.items).toHaveLength(50)
    expect(result.current.offset).toBe(51)
    expect(result.current.reveal).toEqual({ id: '61' })
  })

  it('aborts an off-window cursor probe when scrolling supersedes it', async () => {
    const { result } = renderHook(() => useHistoryWindow(model))
    await act(async () => {})
    const probe = deferred<Awaited<ReturnType<typeof querySearch>>>()
    vi.mocked(querySearch).mockReturnValueOnce(probe.promise)
    let pending!: ReturnType<typeof result.current.ensureNeighbor>
    act(() => {
      pending = result.current.ensureNeighbor('3', 3, 1)
    })
    const signal = vi.mocked(querySearch).mock.calls.at(-1)![1]!
    await act(async () => result.current.reportViewport(120, 124, true))
    expect(signal.aborted).toBe(true)
    await act(async () => {
      probe.resolve(page(0, 50))
      await pending
    })
    expect(result.current.offset).toBe(110)
    expect(await pending).toBeNull()
  })

  it('does not let passive idle measurements cancel an explicit selected-neighbor lookup', async () => {
    const { result } = renderHook(() => useHistoryWindow(model))
    await act(async () => {})
    const probe = deferred<Awaited<ReturnType<typeof querySearch>>>()
    vi.mocked(querySearch).mockReturnValueOnce(probe.promise)
    let pending!: ReturnType<typeof result.current.ensureNeighbor>
    act(() => {
      pending = result.current.ensureNeighbor('3', 3, 1)
    })
    const signal = vi.mocked(querySearch).mock.calls.at(-1)![1]!
    act(() => result.current.reportViewport(20, 24, false))
    await act(async () => {
      vi.advanceTimersByTime(750)
    })
    expect(signal.aborted).toBe(false)
    await act(async () => {
      probe.resolve(page(0, 50))
      await pending
    })
    expect(await pending).toMatchObject({ item: { id: '4' }, index: 4 })
  })
})
