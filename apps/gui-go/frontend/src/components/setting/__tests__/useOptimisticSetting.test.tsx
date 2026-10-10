import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { useOptimisticSetting } from '@/components/setting/useOptimisticSetting'

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

describe('useOptimisticSetting', () => {
  it('shows the pending value immediately and returns to the persisted source on success', async () => {
    const request = deferred()
    const persist = vi.fn(() => request.promise)
    const { result, rerender } = renderHook(
      ({ current }) => useOptimisticSetting<boolean>(current, persist),
      { initialProps: { current: false } }
    )

    act(() => result.current[1](true))
    expect(result.current[0]).toBe(true)

    rerender({ current: true })
    await act(async () => request.resolve())
    expect(result.current[0]).toBe(true)
  })

  it('does not let an older request completion clear a newer optimistic value', async () => {
    const first = deferred()
    const second = deferred()
    const persist = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const { result, rerender } = renderHook(
      ({ current }) => useOptimisticSetting(current, persist),
      { initialProps: { current: 'persisted' } }
    )

    act(() => result.current[1]('first'))
    act(() => result.current[1]('second'))
    await act(async () => first.resolve())

    expect(result.current[0]).toBe('second')

    rerender({ current: 'second' })
    await act(async () => second.resolve())
    expect(result.current[0]).toBe('second')
  })

  it('preserves a pending null value', () => {
    const request = deferred()
    const persist = vi.fn(() => request.promise)
    const { result } = renderHook(() => useOptimisticSetting<string | null>('manual', persist))

    act(() => result.current[1](null))

    expect(result.current[0]).toBeNull()
  })

  it('rolls back only the latest failed mutation to the current persisted value', async () => {
    const request = deferred()
    const persist = vi.fn(() => request.promise)
    const { result, rerender } = renderHook(
      ({ current }) => useOptimisticSetting(current, persist),
      { initialProps: { current: 'stable' } }
    )

    act(() => result.current[1]('pending'))
    rerender({ current: 'external-update' })
    await act(async () => request.reject(new Error('save failed')))

    expect(result.current[0]).toBe('external-update')
  })
})
