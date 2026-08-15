import { useCallback, useEffect, useMemo, useReducer, useRef, type ReactNode } from 'react'
import {
  decideDeviceTrust as submitDecision,
  getDeviceTrust,
  type DeviceTrustChoice,
  type DeviceTrustSnapshot,
} from '@/api/daemon/device-trust'
import { daemonWs } from '@/lib/daemon-ws'
import { DeviceTrustContext } from './device-trust-context'

interface DeviceTrustState {
  snapshot: DeviceTrustSnapshot | null
  loading: boolean
  decisionBusy: boolean
  decisionError: string | null
  localRemovalConfirmationChangeId: string | null
}

type DeviceTrustStateAction =
  | { type: 'refresh_started' }
  | { type: 'refresh_finished'; snapshot: DeviceTrustSnapshot }
  | { type: 'refresh_failed'; error: string }
  | { type: 'decision_started' }
  | {
      type: 'decision_finished'
      snapshot: DeviceTrustSnapshot
      stateChanged: boolean
      localRemovalConfirmationChangeId: string | null
    }
  | { type: 'decision_failed'; error: string }

const initialState: DeviceTrustState = {
  snapshot: null,
  loading: false,
  decisionBusy: false,
  decisionError: null,
  localRemovalConfirmationChangeId: null,
}

function stateReducer(state: DeviceTrustState, action: DeviceTrustStateAction): DeviceTrustState {
  switch (action.type) {
    case 'refresh_started':
      return { ...state, loading: true }
    case 'refresh_finished': {
      const currentChangeId = action.snapshot.currentChange?.changeId ?? null
      return {
        ...state,
        snapshot: action.snapshot,
        loading: false,
        decisionError: null,
        localRemovalConfirmationChangeId:
          state.localRemovalConfirmationChangeId === currentChangeId ? currentChangeId : null,
      }
    }
    case 'refresh_failed':
      return { ...state, loading: false, decisionError: action.error }
    case 'decision_started':
      return { ...state, decisionBusy: true, decisionError: null }
    case 'decision_finished':
      return {
        ...state,
        snapshot: action.snapshot,
        loading: false,
        decisionBusy: false,
        decisionError: action.stateChanged ? 'device_state_changed' : null,
        localRemovalConfirmationChangeId: action.localRemovalConfirmationChangeId,
      }
    case 'decision_failed':
      return { ...state, decisionBusy: false, decisionError: action.error }
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function DeviceTrustProvider({
  enabled,
  children,
}: {
  enabled: boolean
  children: ReactNode
}) {
  const [state, dispatch] = useReducer(stateReducer, initialState)
  const snapshotRef = useRef<DeviceTrustSnapshot | null>(null)
  const decisionBusyRef = useRef(false)
  const refreshSequenceRef = useRef(0)

  const refresh = useCallback(async () => {
    if (!enabled) return
    const sequence = ++refreshSequenceRef.current
    dispatch({ type: 'refresh_started' })
    try {
      const snapshot = await getDeviceTrust()
      if (sequence !== refreshSequenceRef.current) return
      snapshotRef.current = snapshot
      dispatch({ type: 'refresh_finished', snapshot })
    } catch (error) {
      if (sequence !== refreshSequenceRef.current) return
      dispatch({ type: 'refresh_failed', error: errorMessage(error) })
    }
  }, [enabled])

  useEffect(() => {
    if (!enabled) return
    void refresh()
    return daemonWs.subscribe(['device-trust'], () => void refresh())
  }, [enabled, refresh])

  useEffect(() => {
    if (!enabled) return
    const onVisible = () => {
      if (document.visibilityState === 'visible') void refresh()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => document.removeEventListener('visibilitychange', onVisible)
  }, [enabled, refresh])

  const decide = useCallback(
    async (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => {
      const change = snapshotRef.current?.currentChange
      if (!change || decisionBusyRef.current || !change.allowedChoices.includes(choice)) return
      decisionBusyRef.current = true
      refreshSequenceRef.current += 1
      dispatch({ type: 'decision_started' })
      try {
        const result = await submitDecision(change.changeId, choice, confirmLocalRemoval)
        refreshSequenceRef.current += 1
        snapshotRef.current = result.snapshot
        dispatch({
          type: 'decision_finished',
          snapshot: result.snapshot,
          stateChanged: result.kind === 'state_changed',
          localRemovalConfirmationChangeId:
            result.kind === 'local_device_confirmation_required' ? result.changeId : null,
        })
      } catch (error) {
        await refresh()
        dispatch({ type: 'decision_failed', error: errorMessage(error) })
      } finally {
        decisionBusyRef.current = false
      }
    },
    [refresh]
  )

  const value = useMemo(() => ({ ...state, refresh, decide }), [state, refresh, decide])
  return <DeviceTrustContext value={value}>{children}</DeviceTrustContext>
}
