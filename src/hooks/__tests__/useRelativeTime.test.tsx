import { renderHook, act } from '@testing-library/react'
import { formatRelativeTime, useNow } from '@/hooks/useRelativeTime'

/** Identity-ish translator so we can assert on the chosen key + interpolation. */
const t = (key: string, opts?: Record<string, unknown>) =>
  opts ? `${key}:${JSON.stringify(opts)}` : key

describe('formatRelativeTime', () => {
  const now = 1_700_000_000_000

  it('renders "just now" under half a minute (rounds to 0)', () => {
    expect(formatRelativeTime(now - 20_000, now, t)).toBe('clipboard.time.justNow')
  })

  it('renders minutes for < 1 hour', () => {
    expect(formatRelativeTime(now - 5 * 60_000, now, t)).toBe(
      'clipboard.time.minutesAgo:{"minutes":5}'
    )
  })

  it('renders hours for < 1 day', () => {
    expect(formatRelativeTime(now - 3 * 3_600_000, now, t)).toBe(
      'clipboard.time.hoursAgo:{"hours":3}'
    )
  })

  it('renders days beyond 24 hours', () => {
    expect(formatRelativeTime(now - 2 * 86_400_000, now, t)).toBe(
      'clipboard.time.daysAgo:{"days":2}'
    )
  })
})

describe('useNow shared clock', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('advances every 30s while subscribed', () => {
    const { result } = renderHook(() => useNow())
    const first = result.current

    act(() => {
      vi.advanceTimersByTime(30_000)
    })
    expect(result.current).toBeGreaterThan(first)
  })

  it('runs a single timer for many subscribers and stops it once all unmount', () => {
    const setInterval = vi.spyOn(globalThis, 'setInterval')
    const clearInterval = vi.spyOn(globalThis, 'clearInterval')

    const a = renderHook(() => useNow())
    const b = renderHook(() => useNow())
    // One shared timer regardless of subscriber count.
    expect(setInterval).toHaveBeenCalledTimes(1)

    a.unmount()
    // Still one subscriber left → timer kept alive.
    expect(clearInterval).not.toHaveBeenCalled()

    b.unmount()
    // Last subscriber gone → timer torn down.
    expect(clearInterval).toHaveBeenCalledTimes(1)
  })
})
