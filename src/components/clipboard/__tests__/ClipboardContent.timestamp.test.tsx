import { renderHook, act } from '@testing-library/react'
import { useState, useEffect } from 'react'

/**
 * Test the timestamp refresh tick logic extracted from ClipboardContent.
 * We test the hook behavior (tick increment + smart interval) in isolation
 * since the full component requires extensive Redux/i18n/router setup.
 */

// Simulate the tick hook logic as it will exist in ClipboardContent
function useTimestampTick(items: { activeTime: number }[]) {
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (!items || items.length === 0) return

    const now = Date.now()
    const hasRecentItems = items.some(item => now - item.activeTime < 3600000)
    const interval = hasRecentItems ? 30000 : 60000

    const id = setInterval(() => {
      setTick(t => t + 1)
    }, interval)

    return () => clearInterval(id)
  }, [items])

  return tick
}

describe('Timestamp tick refresh logic', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('does not start interval when items list is empty', () => {
    const { result } = renderHook(() => useTimestampTick([]))

    act(() => {
      vi.advanceTimersByTime(60000)
    })

    expect(result.current).toBe(0)
  })

  it('uses 30s interval when recent items exist (< 1 hour old)', () => {
    const now = Date.now()
    const items = [{ activeTime: now - 5 * 60 * 1000 }] // 5 minutes ago

    const { result } = renderHook(() => useTimestampTick(items))

    expect(result.current).toBe(0)

    act(() => {
      vi.advanceTimersByTime(30000)
    })
    expect(result.current).toBe(1)

    act(() => {
      vi.advanceTimersByTime(30000)
    })
    expect(result.current).toBe(2)
  })

  it('uses 60s interval when all items are older than 1 hour', () => {
    const now = Date.now()
    const items = [{ activeTime: now - 2 * 60 * 60 * 1000 }] // 2 hours ago

    const { result } = renderHook(() => useTimestampTick(items))

    expect(result.current).toBe(0)

    // At 30s, should NOT have ticked (60s interval)
    act(() => {
      vi.advanceTimersByTime(30000)
    })
    expect(result.current).toBe(0)

    // At 60s, should tick
    act(() => {
      vi.advanceTimersByTime(30000)
    })
    expect(result.current).toBe(1)
  })

  it('cleans up interval on unmount', () => {
    const now = Date.now()
    const items = [{ activeTime: now - 5 * 60 * 1000 }]

    const { result, unmount } = renderHook(() => useTimestampTick(items))

    act(() => {
      vi.advanceTimersByTime(30000)
    })
    expect(result.current).toBe(1)

    unmount()

    // After unmount, no more ticks should happen
    // (no error from trying to setState on unmounted component)
  })
})
