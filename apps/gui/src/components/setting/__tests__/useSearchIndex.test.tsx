import { act, cleanup, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSearchStatus, triggerSearchRebuild } from '@/api/daemon'
import { useSearchIndex } from '../useSearchIndex'

vi.mock('@/api/daemon', () => ({ getSearchStatus: vi.fn(), triggerSearchRebuild: vi.fn() }))
const ready: Awaited<ReturnType<typeof getSearchStatus>> = {
  data: {
    state: 'ready',
    reason: null,
    lastRebuildStartedAtMs: null,
    lastRebuildCompletedAtMs: null,
  },
  ts: 0,
}
const rebuilding: typeof ready = { ...ready, data: { ...ready.data, state: 'rebuilding' } }
beforeEach(() => {
  vi.useFakeTimers()
  vi.resetAllMocks()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('search index monitoring', () => {
  it('keeps rebuild disabled until the first refreshed status arrives', async () => {
    vi.mocked(getSearchStatus)
      .mockResolvedValueOnce(ready)
      .mockImplementation(() => new Promise(() => {}))
    vi.mocked(triggerSearchRebuild).mockResolvedValue(undefined)
    const { result } = renderHook(useSearchIndex)
    await act(async () => {})
    await act(async () => {
      await result.current.rebuild()
    })
    expect(result.current.rebuilding).toBe(true)
    await act(async () => {
      await result.current.rebuild()
    })
    expect(triggerSearchRebuild).toHaveBeenCalledTimes(1)
  })
  it('resumes an existing rebuild and waits for each request before polling again', async () => {
    let finish!: (value: typeof ready) => void
    vi.mocked(getSearchStatus)
      .mockResolvedValueOnce(rebuilding)
      .mockImplementationOnce(
        () =>
          new Promise(resolve => {
            finish = resolve
          })
      )
    const { result } = renderHook(useSearchIndex)
    await act(async () => {})
    expect(result.current.rebuilding).toBe(true)
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000)
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000)
    })
    expect(getSearchStatus).toHaveBeenCalledTimes(2)
    await act(async () => finish(ready))
    expect(result.current.rebuilding).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('does not start polling after leaving during a rebuild request', async () => {
    let finish!: () => void
    vi.mocked(getSearchStatus).mockResolvedValue(ready)
    vi.mocked(triggerSearchRebuild).mockImplementation(
      () =>
        new Promise(resolve => {
          finish = () => resolve(undefined)
        })
    )
    const { result, unmount } = renderHook(useSearchIndex)
    await act(async () => {})
    act(() => {
      void result.current.rebuild()
    })
    unmount()
    await act(async () => finish())
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000)
    })
    expect(getSearchStatus).toHaveBeenCalledTimes(1)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('stops on a query failure and permits a retry', async () => {
    vi.mocked(getSearchStatus).mockRejectedValueOnce(new Error('offline')).mockResolvedValue(ready)
    vi.mocked(triggerSearchRebuild).mockResolvedValue(undefined)
    const { result } = renderHook(useSearchIndex)
    await act(async () => {})
    expect(result.current.rebuilding).toBe(false)
    await act(async () => {
      await result.current.rebuild()
    })
    expect(result.current.status?.state).toBe('ready')
  })
})
