import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useContentUnlocked } from '../useContentUnlocked'

const mocks = vi.hoisted(() => ({
  query: vi.fn(),
  events: new Map<string, () => void>(),
  encryption: null as (() => void) | null,
}))
vi.mock('@/lib/ipc', () => ({ commands: { getContentUnlocked: mocks.query } }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, handler: () => void) => {
    mocks.events.set(event, handler)
    return Promise.resolve(() => mocks.events.delete(event))
  },
}))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: (_: string[], handler: () => void) => {
      mocks.encryption = handler
      return () => {
        mocks.encryption = null
      }
    },
  },
}))

beforeEach(() => {
  mocks.query.mockReset().mockResolvedValue(false)
  mocks.events.clear()
})

describe('host-owned content lock', () => {
  it('starts closed and never treats background readiness as authentication', async () => {
    const { result } = renderHook(() => useContentUnlocked())
    expect(result.current.unlocked).toBeNull()
    await waitFor(() => expect(result.current.unlocked).toBe(false))
    await act(async () => mocks.encryption?.())
    expect(result.current.unlocked).toBe(false)
  })

  it('rechecks authority on window events instead of trusting event payloads', async () => {
    const { result } = renderHook(() => useContentUnlocked())
    await waitFor(() => expect(result.current.unlocked).toBe(false))
    await act(async () => mocks.events.get('content-lock-changed')?.())
    expect(result.current.unlocked).toBe(false)
    mocks.query.mockResolvedValue(true)
    await act(async () => mocks.events.get('content-lock-changed')?.())
    expect(result.current.unlocked).toBe(true)
    mocks.query.mockRejectedValue(new Error('offline'))
    await act(async () => mocks.events.get('content-lock-changed')?.())
    expect(result.current.unlocked).toBe(false)
  })

  it('does not contact the daemon before bootstrap completes', () => {
    const { result } = renderHook(() => useContentUnlocked(false))
    expect(result.current.unlocked).toBeNull()
    expect(mocks.query).not.toHaveBeenCalled()
  })

  it('hides a previously unlocked result until the re-enabled daemon answers', async () => {
    mocks.query.mockResolvedValue(true)
    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) => useContentUnlocked(enabled),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.unlocked).toBe(true))

    rerender({ enabled: false })
    expect(result.current.unlocked).toBeNull()

    let resolveReconnected: ((value: boolean) => void) | undefined
    const reconnected = new Promise<boolean>(resolve => {
      resolveReconnected = resolve
    })
    mocks.query.mockImplementation(() => reconnected)
    rerender({ enabled: true })
    expect(result.current.unlocked).toBeNull()

    await act(async () => resolveReconnected?.(true))
    expect(result.current.unlocked).toBe(true)
  })

  it('keeps a late request from an old daemon generation from replacing the new result', async () => {
    mocks.query.mockResolvedValue(true)
    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) => useContentUnlocked(enabled),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.unlocked).toBe(true))

    let resolveOld: ((value: boolean) => void) | undefined
    mocks.query.mockImplementationOnce(
      () =>
        new Promise(resolve => {
          resolveOld = resolve
        })
    )
    await act(async () => mocks.encryption?.())

    rerender({ enabled: false })
    mocks.query.mockResolvedValue(false)
    rerender({ enabled: true })
    await waitFor(() => expect(result.current.unlocked).toBe(false))

    await act(async () => resolveOld?.(true))
    expect(result.current.unlocked).toBe(false)
  })

  it('fails closed when the first query after reconnect fails', async () => {
    mocks.query.mockResolvedValue(true)
    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) => useContentUnlocked(enabled),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.unlocked).toBe(true))

    rerender({ enabled: false })
    mocks.query.mockRejectedValue(new Error('offline'))
    rerender({ enabled: true })
    expect(result.current.unlocked).toBeNull()
    await waitFor(() => expect(result.current.unlocked).toBe(false))
  })

  it('invalidates synchronously on refresh and rejects an earlier event response', async () => {
    mocks.query.mockResolvedValue(true)
    const { result } = renderHook(() => useContentUnlocked(true))
    await waitFor(() => expect(result.current.unlocked).toBe(true))

    let resolveOld: ((value: boolean) => void) | undefined
    mocks.query.mockImplementationOnce(
      () =>
        new Promise(resolve => {
          resolveOld = resolve
        })
    )
    await act(async () => mocks.encryption?.())

    let resolveRefreshed: ((value: boolean) => void) | undefined
    const refreshed = new Promise<boolean>(resolve => {
      resolveRefreshed = resolve
    })
    mocks.query.mockImplementation(() => refreshed)
    act(() => result.current.refresh())
    expect(result.current.unlocked).toBeNull()

    await act(async () => resolveRefreshed?.(false))
    expect(result.current.unlocked).toBe(false)
    await act(async () => resolveOld?.(true))
    expect(result.current.unlocked).toBe(false)
  })

  it('keeps a late periodic query from overwriting a newer focus result', async () => {
    vi.useFakeTimers()
    try {
      mocks.query.mockResolvedValue(true)
      const { result } = renderHook(() => useContentUnlocked(true))
      await act(async () => {
        await Promise.resolve()
        await Promise.resolve()
      })
      expect(result.current.unlocked).toBe(true)

      let resolvePeriodic: ((value: boolean) => void) | undefined
      mocks.query.mockImplementationOnce(
        () =>
          new Promise(resolve => {
            resolvePeriodic = resolve
          })
      )
      act(() => vi.advanceTimersByTime(5_000))

      mocks.query.mockResolvedValue(false)
      await act(async () => window.dispatchEvent(new Event('focus')))
      expect(result.current.unlocked).toBe(false)

      await act(async () => resolvePeriodic?.(true))
      expect(result.current.unlocked).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })
})
