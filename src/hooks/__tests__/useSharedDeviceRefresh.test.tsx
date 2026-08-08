/**
 * useSharedDeviceRefresh behavior tests.
 *
 * Covers the Engine-owned snapshot contract: one in-flight full query per
 * request, whole-snapshot replacement, stale-response rejection, event-driven
 * re-querying, reconnect/visibility catch-up, and the connected→device-list
 * hand-off. All assertions observe public behavior through the hook's return
 * values and the mocked daemon API, never internal state.
 */

import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  getSharedDeviceRefresh,
  isSharedDeviceRefreshNotFound,
  startSharedDeviceRefresh,
  type SharedDeviceRefreshSnapshot,
} from '@/api/daemon/member'
import { useSharedDeviceRefresh } from '@/hooks/useSharedDeviceRefresh'

vi.mock('@/api/daemon/member', () => ({
  startSharedDeviceRefresh: vi.fn(),
  getSharedDeviceRefresh: vi.fn(),
  isSharedDeviceRefreshNotFound: vi.fn(() => false),
}))

type DaemonEventHandler = (event: { topic: string; eventType: string; payload: unknown }) => void

const wsHandlers = vi.hoisted(() => ({
  subscribe: vi.fn((_topics: string[], _handler: DaemonEventHandler) => () => undefined),
  onReconnect: vi.fn((_handler: () => void) => () => undefined),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: wsHandlers.subscribe,
    onReconnect: wsHandlers.onReconnect,
  },
}))

const makeSnapshot = (
  overrides: Partial<SharedDeviceRefreshSnapshot> = {}
): SharedDeviceRefreshSnapshot => ({
  requestId: 'refresh-1',
  phase: 'connecting',
  devices: [],
  totalCount: 0,
  discoveredCount: 0,
  connectingCount: 0,
  connectedCount: 0,
  alreadyPresentCount: 0,
  waitingForPeerCount: 0,
  waitingForUpdateCount: 0,
  versionIncompatibleCount: 0,
  rejectedCount: 0,
  unavailableSourceCount: 0,
  ...overrides,
})

const device = (id: string, state: SharedDeviceRefreshSnapshot['devices'][number]['state']) => ({
  deviceId: id,
  displayName: `Device ${id}`,
  state,
})

function renderRefresh() {
  const onDevicesConnected = vi.fn()
  const onStartFailed = vi.fn()
  const result = renderHook(() =>
    useSharedDeviceRefresh({
      onDevicesConnected,
      onStartFailed,
      documentVisible: true,
    })
  )
  return { ...result, onDevicesConnected, onStartFailed }
}

/** Fires the latest shared-device-refresh change notification. */
function emitChanged(requestId: string) {
  const handler = wsHandlers.subscribe.mock.calls.at(-1)?.[1]
  expect(handler).toBeTypeOf('function')
  act(() => {
    handler?.({
      topic: 'shared-device-refresh',
      eventType: 'shared-device-refresh.changed',
      payload: { requestId },
    })
  })
}

/** Fires the global signal used when an event consumer has missed updates. */
function emitRefreshRequired() {
  const handler = wsHandlers.subscribe.mock.calls.at(-1)?.[1]
  expect(handler).toBeTypeOf('function')
  act(() => {
    handler?.({
      topic: 'system',
      eventType: 'system.refresh_required',
      payload: {},
    })
  })
}

/** Fires the latest reconnect callback registered by the hook. */
function emitReconnect() {
  const handler = wsHandlers.onReconnect.mock.calls.at(-1)?.[0]
  expect(handler).toBeTypeOf('function')
  act(() => {
    handler?.()
  })
}

/** Waits until every scheduled query for the current round has settled. */
async function settleQueries() {
  await act(async () => {})
}

describe('useSharedDeviceRefresh', () => {
  beforeEach(() => {
    vi.mocked(startSharedDeviceRefresh).mockResolvedValue('refresh-1')
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(makeSnapshot())
    vi.mocked(isSharedDeviceRefreshNotFound).mockReturnValue(false)
    wsHandlers.subscribe.mockClear()
    wsHandlers.onReconnect.mockClear()
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('starts a refresh and immediately queries the full snapshot', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })

    await waitFor(() => expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(getSharedDeviceRefresh).toHaveBeenCalledWith('refresh-1'))
    expect(result.current.requestId).toBe('refresh-1')
    await waitFor(() => expect(result.current.snapshot?.requestId).toBe('refresh-1'))
  })

  it('reuses the in-flight request when the entry is clicked again', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.requestId).toBe('refresh-1'))

    act(() => {
      void result.current.startRefresh()
    })

    expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(1)
    expect(result.current.open).toBe(true)
  })

  it('starts a new round once the first round has completed', async () => {
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ phase: 'round_completed', devices: [] })
    )
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot?.phase).toBe('round_completed'))
    await settleQueries()

    vi.mocked(startSharedDeviceRefresh).mockResolvedValue('refresh-2')
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ requestId: 'refresh-2', phase: 'started' })
    )

    act(() => {
      void result.current.startRefresh()
    })

    await waitFor(() => expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(result.current.requestId).toBe('refresh-2'))
    expect(result.current.snapshot?.requestId).toBe('refresh-2')
  })

  it('replaces the whole snapshot instead of merging partial results', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()

    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({
        phase: 'connecting',
        devices: [device('peer-1', 'connecting')],
        totalCount: 1,
        connectingCount: 1,
      })
    )
    emitChanged('refresh-1')
    await waitFor(() =>
      expect(result.current.snapshot?.devices.map(d => d.deviceId)).toEqual(['peer-1'])
    )

    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({
        phase: 'round_completed',
        devices: [device('peer-1', 'connected'), device('peer-2', 'waiting_for_peer')],
        totalCount: 2,
        connectedCount: 1,
        waitingForPeerCount: 1,
      })
    )
    emitChanged('refresh-1')

    await waitFor(() =>
      expect(result.current.snapshot?.devices.map(d => `${d.deviceId}:${d.state}`)).toEqual([
        'peer-1:connected',
        'peer-2:waiting_for_peer',
      ])
    )
    expect(result.current.snapshot?.devices).toHaveLength(2)
  })

  it('ignores a stale response from the previous round', async () => {
    const pending = new Map<string, (s: SharedDeviceRefreshSnapshot) => void>()
    vi.mocked(getSharedDeviceRefresh).mockImplementation(
      (requestId: string) =>
        new Promise<SharedDeviceRefreshSnapshot>(resolve => pending.set(requestId, resolve))
    )
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(getSharedDeviceRefresh).toHaveBeenCalledWith('refresh-1'))

    // The first round completes and its follow-up query stays in flight.
    act(() => {
      pending.get('refresh-1')?.(makeSnapshot({ phase: 'round_completed', devices: [] }))
    })
    await waitFor(() => expect(result.current.snapshot?.phase).toBe('round_completed'))
    await settleQueries()

    // Close the finished round and start a new one; the new query is in flight.
    act(() => {
      result.current.closeRefresh()
    })
    expect(result.current.requestId).toBeNull()

    vi.mocked(startSharedDeviceRefresh).mockResolvedValue('refresh-2')
    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.requestId).toBe('refresh-2'))
    expect(getSharedDeviceRefresh).toHaveBeenCalledWith('refresh-2')

    // A stale response for the old request lands late; it must be rejected.
    act(() => {
      pending.get('refresh-1')?.(makeSnapshot({ requestId: 'refresh-1', phase: 'connecting' }))
    })

    expect(result.current.requestId).toBe('refresh-2')
    expect(result.current.snapshot).toBeNull()

    act(() => {
      pending.get('refresh-2')?.(makeSnapshot({ requestId: 'refresh-2', phase: 'connecting' }))
    })
    await waitFor(() => expect(result.current.snapshot?.requestId).toBe('refresh-2'))
  })

  it('keeps the last valid snapshot on a temporary failure and recovers on the next event', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()

    vi.mocked(getSharedDeviceRefresh).mockRejectedValueOnce(new Error('daemon offline'))
    emitChanged('refresh-1')

    await waitFor(() => expect(result.current.queryFailure).toBe('temporary'))
    expect(result.current.snapshot).not.toBeNull()

    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ phase: 'round_completed', devices: [] })
    )
    emitChanged('refresh-1')

    await waitFor(() => expect(result.current.queryFailure).toBeNull())
    expect(result.current.snapshot?.phase).toBe('round_completed')
  })

  it('treats a 404 as a finished refresh', async () => {
    vi.mocked(isSharedDeviceRefreshNotFound).mockReturnValue(true)
    vi.mocked(getSharedDeviceRefresh).mockRejectedValue(new Error('not found'))
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })

    await waitFor(() => expect(result.current.queryFailure).toBe('notFound'))
    expect(result.current.snapshot).toBeNull()
  })

  it('queues at most one follow-up query while a query is in flight', async () => {
    const pending: Array<() => void> = []
    vi.mocked(getSharedDeviceRefresh).mockImplementation(
      () =>
        new Promise<SharedDeviceRefreshSnapshot>(resolve =>
          pending.push(() => resolve(makeSnapshot()))
        )
    )
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(getSharedDeviceRefresh).toHaveBeenCalledTimes(1))

    // Two change events while the first query is in flight: only one follow-up
    // query may run afterwards.
    emitChanged('refresh-1')
    emitChanged('refresh-1')

    act(() => {
      pending.shift()?.()
    })

    await waitFor(() => expect(getSharedDeviceRefresh).toHaveBeenCalledTimes(2))

    act(() => {
      pending.shift()?.()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    expect(getSharedDeviceRefresh).toHaveBeenCalledTimes(2)
    expect(result.current.snapshot?.requestId).toBe('refresh-1')
  })

  it('ignores change events for a different request', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()
    const callsBefore = vi.mocked(getSharedDeviceRefresh).mock.calls.length

    emitChanged('some-other-request')
    await settleQueries()

    expect(vi.mocked(getSharedDeviceRefresh).mock.calls.length).toBe(callsBefore)
  })

  it('re-queries after a realtime reconnect', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()
    const callsBefore = vi.mocked(getSharedDeviceRefresh).mock.calls.length

    emitReconnect()

    await waitFor(() =>
      expect(vi.mocked(getSharedDeviceRefresh).mock.calls.length).toBeGreaterThan(callsBefore)
    )
  })

  it('re-queries after the daemon reports that realtime updates were missed', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()
    const callsBefore = vi.mocked(getSharedDeviceRefresh).mock.calls.length
    expect(wsHandlers.subscribe).toHaveBeenLastCalledWith(
      ['shared-device-refresh', 'system'],
      expect.any(Function)
    )

    emitRefreshRequired()

    await waitFor(() =>
      expect(vi.mocked(getSharedDeviceRefresh).mock.calls.length).toBeGreaterThan(callsBefore)
    )
  })

  it('re-queries when the document becomes visible again', async () => {
    const props = { onDevicesConnected: vi.fn(), onStartFailed: vi.fn(), documentVisible: true }
    const { result, rerender } = renderHook(useSharedDeviceRefresh, { initialProps: props })

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()
    const callsBefore = vi.mocked(getSharedDeviceRefresh).mock.calls.length

    rerender({ ...props, documentVisible: false })
    await settleQueries()
    rerender({ ...props, documentVisible: true })

    await waitFor(() =>
      expect(vi.mocked(getSharedDeviceRefresh).mock.calls.length).toBeGreaterThan(callsBefore)
    )
  })

  it('notifies once when a device first becomes connected', async () => {
    const { result, onDevicesConnected } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()

    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ devices: [device('peer-1', 'connected')], connectedCount: 1 })
    )
    emitChanged('refresh-1')

    await waitFor(() => expect(onDevicesConnected).toHaveBeenCalledTimes(1))
  })

  it('does not notify again when the same device stays connected', async () => {
    const { result, onDevicesConnected } = renderRefresh()
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ devices: [device('peer-1', 'connected')], connectedCount: 1 })
    )

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(onDevicesConnected).toHaveBeenCalledTimes(1))
    await settleQueries()

    emitChanged('refresh-1')
    await waitFor(() =>
      expect(vi.mocked(getSharedDeviceRefresh).mock.calls.length).toBeGreaterThan(1)
    )

    expect(onDevicesConnected).toHaveBeenCalledTimes(1)
  })

  it('closes the window and reports when starting fails', async () => {
    vi.mocked(startSharedDeviceRefresh).mockRejectedValue(new Error('daemon offline'))
    const { result, onStartFailed } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })

    await waitFor(() => expect(onStartFailed).toHaveBeenCalledTimes(1))
    expect(result.current.open).toBe(false)
    expect(result.current.requestId).toBeNull()
  })

  it('keeps working in the background after the window is closed mid-round', async () => {
    const { result, onDevicesConnected } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()

    act(() => {
      result.current.closeRefresh()
    })
    expect(result.current.open).toBe(false)
    expect(result.current.requestId).toBe('refresh-1')

    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ devices: [device('peer-1', 'connected')], connectedCount: 1 })
    )
    emitChanged('refresh-1')

    await waitFor(() => expect(onDevicesConnected).toHaveBeenCalledTimes(1))
    expect(result.current.snapshot?.devices[0].state).toBe('connected')
  })

  it.each(['waiting_for_peer', 'waiting_for_update'] as const)(
    'keeps following a completed round while a device is %s',
    async waitingState => {
      vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
        makeSnapshot({
          phase: 'round_completed',
          devices: [device('peer-1', waitingState)],
          totalCount: 1,
          waitingForPeerCount: waitingState === 'waiting_for_peer' ? 1 : 0,
          waitingForUpdateCount: waitingState === 'waiting_for_update' ? 1 : 0,
        })
      )
      const { result, onDevicesConnected } = renderRefresh()

      act(() => {
        void result.current.startRefresh()
      })
      await waitFor(() => expect(result.current.snapshot?.phase).toBe('round_completed'))

      act(() => {
        result.current.closeRefresh()
      })
      expect(result.current.open).toBe(false)
      expect(result.current.requestId).toBe('refresh-1')

      vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
        makeSnapshot({
          phase: 'round_completed',
          devices: [device('peer-1', 'connected')],
          totalCount: 1,
          connectedCount: 1,
        })
      )
      emitChanged('refresh-1')

      await waitFor(() => expect(onDevicesConnected).toHaveBeenCalledTimes(1))
      expect(result.current.snapshot?.devices[0].state).toBe('connected')
    }
  )

  it('keeps the new round serialized when an old-round query finishes late', async () => {
    type PendingQuery = {
      requestId: string
      resolve: (snapshot: SharedDeviceRefreshSnapshot) => void
    }
    const pending: PendingQuery[] = []
    vi.mocked(getSharedDeviceRefresh).mockImplementation(
      requestId =>
        new Promise<SharedDeviceRefreshSnapshot>(resolve => pending.push({ requestId, resolve }))
    )
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(pending).toHaveLength(1))
    act(() => {
      pending.shift()?.resolve(makeSnapshot({ phase: 'round_completed' }))
    })
    await waitFor(() => expect(result.current.snapshot?.phase).toBe('round_completed'))

    emitChanged('refresh-1')
    await waitFor(() => expect(pending).toHaveLength(1))

    act(() => {
      result.current.closeRefresh()
    })
    vi.mocked(startSharedDeviceRefresh).mockResolvedValue('refresh-2')
    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() =>
      expect(pending.filter(query => query.requestId === 'refresh-2')).toHaveLength(1)
    )

    emitChanged('refresh-2')
    const oldQuery = pending.find(query => query.requestId === 'refresh-1')
    act(() => {
      oldQuery?.resolve(makeSnapshot({ requestId: 'refresh-1', phase: 'round_completed' }))
    })
    await settleQueries()

    emitChanged('refresh-2')
    await settleQueries()

    expect(pending.filter(query => query.requestId === 'refresh-2')).toHaveLength(1)
  })

  it('reopens the same window when closed mid-round and the entry is clicked', async () => {
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot).not.toBeNull())
    await settleQueries()

    act(() => {
      result.current.closeRefresh()
    })
    expect(result.current.open).toBe(false)

    act(() => {
      void result.current.startRefresh()
    })

    expect(startSharedDeviceRefresh).toHaveBeenCalledTimes(1)
    expect(result.current.open).toBe(true)
  })

  it('clears the request after closing a completed round', async () => {
    vi.mocked(getSharedDeviceRefresh).mockResolvedValue(
      makeSnapshot({ phase: 'round_completed', devices: [] })
    )
    const { result } = renderRefresh()

    act(() => {
      void result.current.startRefresh()
    })
    await waitFor(() => expect(result.current.snapshot?.phase).toBe('round_completed'))

    act(() => {
      result.current.closeRefresh()
    })

    expect(result.current.requestId).toBeNull()
    expect(result.current.snapshot).toBeNull()
  })
})
