import { useEffect, useReducer, useRef } from 'react'
import {
  getSharedDeviceRefresh,
  isSharedDeviceRefreshNotFound,
  startSharedDeviceRefresh,
  type SharedDeviceRefreshSnapshot,
} from '@/api/daemon/member'
import { daemonWs } from '@/lib/daemon-ws'

export type SharedDeviceRefreshQueryFailure = 'temporary' | 'notFound'

interface SharedDeviceRefreshState {
  open: boolean
  starting: boolean
  requestId: string | null
  snapshot: SharedDeviceRefreshSnapshot | null
  queryFailure: SharedDeviceRefreshQueryFailure | null
}

type SharedDeviceRefreshAction =
  | { type: 'open' }
  | { type: 'begin' }
  | { type: 'started'; requestId: string }
  | { type: 'snapshot'; snapshot: SharedDeviceRefreshSnapshot }
  | { type: 'queryFailed'; failure: SharedDeviceRefreshQueryFailure }
  | { type: 'close' }

const INITIAL_STATE: SharedDeviceRefreshState = {
  open: false,
  starting: false,
  requestId: null,
  snapshot: null,
  queryFailure: null,
}

function hasDeferredDevices(snapshot: SharedDeviceRefreshSnapshot): boolean {
  return snapshot.devices.some(
    device => device.state === 'waiting_for_peer' || device.state === 'waiting_for_update'
  )
}

function isTerminal(state: SharedDeviceRefreshState): boolean {
  if (state.queryFailure === 'notFound') return true
  return state.snapshot?.phase === 'round_completed' && !hasDeferredDevices(state.snapshot)
}

function reducer(
  state: SharedDeviceRefreshState,
  action: SharedDeviceRefreshAction
): SharedDeviceRefreshState {
  switch (action.type) {
    case 'open':
      return { ...state, open: true }
    case 'begin':
      return {
        ...state,
        open: true,
        starting: true,
        requestId: null,
        snapshot: null,
        queryFailure: null,
      }
    case 'started':
      return { ...state, starting: false, requestId: action.requestId }
    case 'snapshot':
      if (action.snapshot.requestId !== state.requestId) return state
      return { ...state, snapshot: action.snapshot, queryFailure: null }
    case 'queryFailed':
      if (action.failure === 'notFound') {
        return { ...state, queryFailure: 'notFound', snapshot: null }
      }
      return { ...state, queryFailure: 'temporary' }
    case 'close':
      if (isTerminal(state)) {
        return INITIAL_STATE
      }
      return { ...state, open: false }
    default:
      return state
  }
}

interface UseSharedDeviceRefreshOptions {
  onDevicesConnected: () => void
  onStartFailed: () => void
  documentVisible: boolean
}

/**
 * Owns the shared-device refresh window and the one-in-flight full-snapshot
 * query loop. The request ID stays alive after the window is closed while a
 * round is still active, so clicking the entry later reopens the same Engine
 * result instead of starting duplicate work.
 */
export function useSharedDeviceRefresh({
  onDevicesConnected,
  onStartFailed,
  documentVisible,
}: UseSharedDeviceRefreshOptions) {
  const [state, dispatch] = useReducer(reducer, INITIAL_STATE)

  const requestIdRef = useRef<string | null>(null)
  const startingRef = useRef(false)
  const queryRoundRef = useRef<{
    requestId: string
    inFlight: boolean
    queued: boolean
  } | null>(null)
  const connectedIdsRef = useRef<ReadonlySet<string>>(new Set())
  const onDevicesConnectedRef = useRef(onDevicesConnected)
  const onStartFailedRef = useRef(onStartFailed)
  const runQueryRef = useRef<() => Promise<void>>(async () => {})

  useEffect(() => {
    onDevicesConnectedRef.current = onDevicesConnected
    onStartFailedRef.current = onStartFailed
  })

  useEffect(() => {
    runQueryRef.current = async () => {
      const requestId = requestIdRef.current
      if (!requestId) return

      let queryRound = queryRoundRef.current
      if (!queryRound || queryRound.requestId !== requestId) {
        queryRound = { requestId, inFlight: false, queued: false }
        queryRoundRef.current = queryRound
      }

      if (queryRound.inFlight) {
        queryRound.queued = true
        return
      }

      queryRound.inFlight = true
      try {
        const snapshot = await getSharedDeviceRefresh(requestId)
        if (requestIdRef.current !== requestId) return
        dispatch({ type: 'snapshot', snapshot })

        const nextConnectedIds = new Set(
          snapshot.devices
            .filter(device => device.state === 'connected')
            .map(device => device.deviceId)
        )
        const hasNewConnectedDevice =
          nextConnectedIds.size > connectedIdsRef.current.size ||
          [...nextConnectedIds].some(id => !connectedIdsRef.current.has(id))
        connectedIdsRef.current = nextConnectedIds
        if (hasNewConnectedDevice) {
          onDevicesConnectedRef.current()
        }
      } catch (error) {
        if (requestIdRef.current !== requestId) return
        dispatch({
          type: 'queryFailed',
          failure: isSharedDeviceRefreshNotFound(error) ? 'notFound' : 'temporary',
        })
      } finally {
        queryRound.inFlight = false
        if (
          queryRound.queued &&
          requestIdRef.current === requestId &&
          queryRoundRef.current === queryRound
        ) {
          queryRound.queued = false
          void runQueryRef.current()
        }
      }
    }
  })

  const startRefresh = async () => {
    if (startingRef.current) return

    if (requestIdRef.current) {
      const activeRound = !isTerminal(state)
      if (activeRound) {
        dispatch({ type: 'open' })
        return
      }
    }

    startingRef.current = true
    dispatch({ type: 'begin' })
    try {
      const requestId = await startSharedDeviceRefresh()
      requestIdRef.current = requestId
      queryRoundRef.current = { requestId, inFlight: false, queued: false }
      connectedIdsRef.current = new Set()
      dispatch({ type: 'started', requestId })
      await runQueryRef.current()
    } catch {
      requestIdRef.current = null
      queryRoundRef.current = null
      connectedIdsRef.current = new Set()
      dispatch({ type: 'close' })
      onStartFailedRef.current()
    } finally {
      startingRef.current = false
    }
  }

  const closeRefresh = () => {
    const terminal = isTerminal(state)
    dispatch({ type: 'close' })
    if (terminal) {
      requestIdRef.current = null
      queryRoundRef.current = null
      connectedIdsRef.current = new Set()
    }
  }

  useEffect(() => {
    if (!state.requestId) return
    return daemonWs.subscribe<{ requestId?: string }>(
      ['shared-device-refresh', 'system'],
      event => {
        if (event.eventType === 'system.refresh_required') {
          void runQueryRef.current()
          return
        }
        if (event.eventType !== 'shared-device-refresh.changed') return
        if (event.payload?.requestId !== requestIdRef.current) return
        void runQueryRef.current()
      }
    )
  }, [state.requestId])

  useEffect(() => {
    if (!state.requestId) return
    return daemonWs.onReconnect(() => {
      if (requestIdRef.current) void runQueryRef.current()
    })
  }, [state.requestId])

  useEffect(() => {
    if (!documentVisible || !state.requestId) return
    void runQueryRef.current()
  }, [documentVisible, state.requestId])

  return {
    open: state.open,
    starting: state.starting,
    requestId: state.requestId,
    snapshot: state.snapshot,
    queryFailure: state.queryFailure,
    startRefresh,
    closeRefresh,
  }
}
