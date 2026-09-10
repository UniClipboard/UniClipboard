import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useStartupElapsed } from '@/hooks/useStartupElapsed'

describe('startup elapsed clock', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())
  it('rebases on a newer sample and does not go backwards on stale samples', () => {
    const { result, rerender } = renderHook(
      ({ elapsed }) => useStartupElapsed('one', elapsed, true),
      { initialProps: { elapsed: 20000 } }
    )
    act(() => vi.advanceTimersByTime(2000))
    expect(result.current).toBe(22)
    rerender({ elapsed: 40000 })
    act(() => vi.advanceTimersByTime(1000))
    expect(result.current).toBe(41)
    rerender({ elapsed: 20000 })
    expect(result.current).toBe(41)
  })
  it('resets on another attempt, freezes at termination, and cleans up its timer', () => {
    const { result, rerender, unmount } = renderHook(
      ({ attempt, elapsed, running }) => useStartupElapsed(attempt, elapsed, running),
      { initialProps: { attempt: 'one', elapsed: 20000, running: true } }
    )
    act(() => vi.advanceTimersByTime(2000))
    rerender({ attempt: 'two', elapsed: 0, running: true })
    expect(result.current).toBe(0)
    act(() => vi.advanceTimersByTime(1000))
    rerender({ attempt: 'two', elapsed: 1000, running: false })
    act(() => vi.advanceTimersByTime(5000))
    expect(result.current).toBe(1)
    unmount()
    expect(vi.getTimerCount()).toBe(0)
  })
})
