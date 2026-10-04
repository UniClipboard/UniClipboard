import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SearchParams } from '@/api/daemon/search'
import { useSearchCounts } from '../useSearchCounts'

const q = (query: string): SearchParams[] => [{ query }]

describe('useSearchCounts', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('debounces and returns counts for the current queries', async () => {
    const fetchCounts = vi.fn().mockResolvedValue([3])
    const { result } = renderHook(() => useSearchCounts(q('a'), fetchCounts))

    expect(fetchCounts).not.toHaveBeenCalled()
    await act(() => vi.advanceTimersByTimeAsync(250))

    expect(fetchCounts).toHaveBeenCalledTimes(1)
    expect(result.current).toEqual([3])
  })

  it('only keeps the latest queries when they change before the debounce fires', async () => {
    const fetchCounts = vi.fn().mockResolvedValue([9])
    const { result, rerender } = renderHook(
      ({ queries }) => useSearchCounts(queries, fetchCounts),
      {
        initialProps: { queries: q('a') },
      }
    )

    await act(() => vi.advanceTimersByTimeAsync(100))
    rerender({ queries: q('ab') })
    await act(() => vi.advanceTimersByTimeAsync(250))

    expect(fetchCounts).toHaveBeenCalledTimes(1)
    expect(fetchCounts.mock.calls[0][0]).toEqual(q('ab'))
    expect(result.current).toEqual([9])
  })

  it('drops a response that lands after the queries changed', async () => {
    let resolveFirst: (counts: number[]) => void = () => {}
    const fetchCounts = vi
      .fn()
      .mockImplementationOnce(() => new Promise<number[]>(r => (resolveFirst = r)))
      .mockResolvedValueOnce([2])
    const { result, rerender } = renderHook(
      ({ queries }) => useSearchCounts(queries, fetchCounts),
      {
        initialProps: { queries: q('a') },
      }
    )

    await act(() => vi.advanceTimersByTimeAsync(250))
    rerender({ queries: q('b') })
    await act(async () => resolveFirst([1]))
    expect(result.current).toBeNull()

    await act(() => vi.advanceTimersByTimeAsync(250))
    expect(result.current).toEqual([2])
  })

  it('returns null and stays quiet when counting fails', async () => {
    const fetchCounts = vi.fn().mockRejectedValue(new Error('index_rebuilding'))
    const { result } = renderHook(() => useSearchCounts(q('a'), fetchCounts))

    await act(() => vi.advanceTimersByTimeAsync(250))

    expect(result.current).toBeNull()
  })

  it('never fetches without a fetcher or queries', async () => {
    const fetchCounts = vi.fn()
    renderHook(() => useSearchCounts(null, fetchCounts))
    renderHook(() => useSearchCounts(q('a'), undefined))

    await act(() => vi.advanceTimersByTimeAsync(1000))

    expect(fetchCounts).not.toHaveBeenCalled()
  })
})
