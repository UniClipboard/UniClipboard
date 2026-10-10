import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useProfileRecovery } from '../useProfileRecovery'

const mocks = vi.hoisted(() => ({
  query: vi.fn(),
  notify: null as (() => void) | null,
  stop: vi.fn(),
}))
vi.mock('@/lib/ipc', () => ({ commands: { getProfileRecovery: mocks.query } }))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: (_: string[], handler: () => void) => {
      mocks.notify = handler
      return mocks.stop
    },
  },
}))
beforeEach(() => {
  vi.clearAllMocks()
  mocks.query.mockResolvedValue({ backgroundReady: false })
})
describe('recovery status before setup', () => {
  it('does not query before the connection is ready', () => {
    const { result } = renderHook(() => useProfileRecovery(false))
    expect(result.current.status).toBeNull()
    expect(mocks.query).not.toHaveBeenCalled()
  })
  it('queries on notifications and fails closed if the daemon disappears', async () => {
    const { result, unmount } = renderHook(() => useProfileRecovery(true))
    await waitFor(() => expect(result.current.status?.backgroundReady).toBe(false))
    mocks.query.mockResolvedValue({ backgroundReady: true })
    await act(async () => mocks.notify?.())
    expect(result.current.status?.backgroundReady).toBe(true)
    mocks.query.mockRejectedValue(new Error('offline'))
    await act(async () => mocks.notify?.())
    expect(result.current.status).toBeNull()
    expect(result.current.failed).toBe(true)
    unmount()
    expect(mocks.stop).toHaveBeenCalledOnce()
  })
  it('ignores an older response after a newer notification', async () => {
    let resolveOld: ((value: unknown) => void) | undefined
    mocks.query.mockImplementationOnce(
      () =>
        new Promise(resolve => {
          resolveOld = resolve
        })
    )
    const { result } = renderHook(() => useProfileRecovery(true))
    mocks.query.mockResolvedValue({ backgroundReady: true })
    await act(async () => mocks.notify?.())
    await act(async () => resolveOld?.({ backgroundReady: false }))
    expect(result.current.status?.backgroundReady).toBe(true)
  })

  it('hides a previous ready result until the re-enabled daemon answers', async () => {
    mocks.query.mockResolvedValue({ backgroundReady: true })
    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) => useProfileRecovery(enabled),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.status?.backgroundReady).toBe(true))

    rerender({ enabled: false })
    expect(result.current.status).toBeNull()

    let resolveReconnected: ((value: { backgroundReady: boolean }) => void) | undefined
    mocks.query.mockImplementation(
      () =>
        new Promise(resolve => {
          resolveReconnected = resolve
        })
    )
    rerender({ enabled: true })
    expect(result.current.status).toBeNull()

    await act(async () => resolveReconnected?.({ backgroundReady: true }))
    expect(result.current.status?.backgroundReady).toBe(true)
  })

  it('ignores an old daemon generation after reconnecting', async () => {
    mocks.query.mockResolvedValue({ backgroundReady: true })
    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) => useProfileRecovery(enabled),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.status?.backgroundReady).toBe(true))

    let resolveOld: ((value: { backgroundReady: boolean }) => void) | undefined
    mocks.query.mockImplementationOnce(
      () =>
        new Promise(resolve => {
          resolveOld = resolve
        })
    )
    await act(async () => mocks.notify?.())

    rerender({ enabled: false })
    mocks.query.mockResolvedValue({ backgroundReady: false })
    rerender({ enabled: true })
    await waitFor(() => expect(result.current.status?.backgroundReady).toBe(false))

    await act(async () => resolveOld?.({ backgroundReady: true }))
    expect(result.current.status?.backgroundReady).toBe(false)
  })

  it('reports failure without exposing the previous generation after reconnecting', async () => {
    mocks.query.mockResolvedValue({ backgroundReady: true })
    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) => useProfileRecovery(enabled),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.status?.backgroundReady).toBe(true))

    rerender({ enabled: false })
    mocks.query.mockRejectedValue(new Error('offline'))
    rerender({ enabled: true })
    expect(result.current.status).toBeNull()
    expect(result.current.failed).toBe(false)
    await waitFor(() => expect(result.current.failed).toBe(true))
    expect(result.current.status).toBeNull()
  })

  it('invalidates synchronously on refresh and rejects an earlier notification response', async () => {
    mocks.query.mockResolvedValue({ backgroundReady: true })
    const { result } = renderHook(() => useProfileRecovery(true))
    await waitFor(() => expect(result.current.status?.backgroundReady).toBe(true))

    let resolveOld: ((value: { backgroundReady: boolean }) => void) | undefined
    mocks.query.mockImplementationOnce(
      () =>
        new Promise(resolve => {
          resolveOld = resolve
        })
    )
    await act(async () => mocks.notify?.())

    let resolveRefreshed: ((value: { backgroundReady: boolean }) => void) | undefined
    const refreshed = new Promise<{ backgroundReady: boolean }>(resolve => {
      resolveRefreshed = resolve
    })
    mocks.query.mockImplementation(() => refreshed)
    act(() => result.current.refresh())
    expect(result.current.status).toBeNull()
    expect(result.current.failed).toBe(false)

    await act(async () => resolveRefreshed?.({ backgroundReady: false }))
    expect(result.current.status?.backgroundReady).toBe(false)
    await act(async () => resolveOld?.({ backgroundReady: true }))
    expect(result.current.status?.backgroundReady).toBe(false)
  })
})
