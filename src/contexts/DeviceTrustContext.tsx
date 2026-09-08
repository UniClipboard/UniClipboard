import {
  useCallback,
  useEffect,
  useEffectEvent,
  useMemo,
  useReducer,
  useRef,
  type ReactNode,
} from 'react'
import {
  chooseDeviceGroup,
  getDeviceGroupChoices,
  type DeviceGroupChoiceOutcome,
  type DeviceGroupChoices,
} from '@/api/daemon/device-trust'
import { decisionFingerprint } from '@/components/device/device-group-presentation'
import { DeviceTrustContext, type DeviceGroupDecision } from '@/contexts/device-trust-context'
import { daemonWs } from '@/lib/daemon-ws'

interface DeviceTrustState {
  deviceGroups: DeviceGroupChoices | null
  loading: boolean
  decisionBusy: boolean
  decisionError: string | null
  localRemovalConfirmationIssueId: string | null
  localRemovalConfirmationChoiceId: string | null
  decision: DeviceGroupDecision | null
  acknowledging: boolean
}

type DeviceTrustStateAction =
  | { type: 'refresh_started' }
  | { type: 'refresh_finished'; deviceGroups: DeviceGroupChoices }
  | { type: 'refresh_failed'; error: string }
  | { type: 'choice_started'; decision: DeviceGroupDecision }
  | { type: 'acknowledged' }
  | { type: 'confirmation_cancelled' }
  | { type: 'choice_outdated' }
  | {
      type: 'choice_finished'
      outcome: DeviceGroupChoiceOutcome
      issueId: string
    }
  | { type: 'choice_failed'; error: string }

const initialState: DeviceTrustState = {
  deviceGroups: null,
  loading: false,
  decisionBusy: false,
  decisionError: null,
  localRemovalConfirmationIssueId: null,
  localRemovalConfirmationChoiceId: null,
  decision: null,
  acknowledging: false,
}

function issueStillCurrent(deviceGroups: DeviceGroupChoices, issueId: string | null): boolean {
  return issueId !== null && deviceGroups.issues.some(issue => issue.issueId === issueId)
}

function confirmationStillCurrent(
  previous: DeviceGroupChoices | null,
  current: DeviceGroupChoices,
  issueId: string | null
): boolean {
  const before = previous?.issues.find(issue => issue.issueId === issueId)
  const after = current.issues.find(issue => issue.issueId === issueId)
  return !!before && !!after && decisionFingerprint(before) === decisionFingerprint(after)
}

function stateReducer(state: DeviceTrustState, action: DeviceTrustStateAction): DeviceTrustState {
  switch (action.type) {
    case 'refresh_started':
      return { ...state, loading: true }
    case 'refresh_finished':
      return {
        ...state,
        deviceGroups: action.deviceGroups,
        decision: state.acknowledging ? null : state.decision,
        acknowledging: false,
        loading: false,
        decisionError:
          state.decisionError === 'device_state_changed' && action.deviceGroups.issues.length > 0
            ? state.decisionError
            : null,
        localRemovalConfirmationIssueId: confirmationStillCurrent(
          state.deviceGroups,
          action.deviceGroups,
          state.localRemovalConfirmationIssueId
        )
          ? state.localRemovalConfirmationIssueId
          : null,
      }
    case 'refresh_failed':
      return { ...state, loading: false, acknowledging: false, decisionError: action.error }
    case 'choice_started':
      return { ...state, decisionBusy: true, decisionError: null, decision: action.decision }
    case 'acknowledged':
      return {
        ...state,
        acknowledging: true,
        localRemovalConfirmationIssueId: null,
        localRemovalConfirmationChoiceId: null,
      }
    case 'confirmation_cancelled':
      return {
        ...state,
        localRemovalConfirmationIssueId: null,
        localRemovalConfirmationChoiceId: null,
      }
    case 'choice_outdated':
      return {
        ...state,
        decisionError: 'device_state_changed',
        localRemovalConfirmationIssueId: null,
        localRemovalConfirmationChoiceId: null,
      }
    case 'choice_finished':
      return {
        ...state,
        decisionBusy: false,
        localRemovalConfirmationChoiceId:
          action.outcome === 'local_device_confirmation_required'
            ? (state.decision?.choiceId ?? null)
            : null,
        decision:
          action.outcome === 'state_changed' ||
          action.outcome === 'local_device_confirmation_required'
            ? null
            : state.decision && { ...state.decision, outcome: action.outcome },
        decisionError:
          action.outcome === 'state_changed' ? 'device_state_changed' : state.decisionError,
        localRemovalConfirmationIssueId:
          action.outcome === 'local_device_confirmation_required' &&
          state.deviceGroups !== null &&
          issueStillCurrent(state.deviceGroups, action.issueId)
            ? action.issueId
            : null,
      }
    case 'choice_failed':
      return {
        ...state,
        decisionBusy: false,
        decisionError: action.error,
        decision: state.decision && { ...state.decision, outcome: 'uncertain' },
      }
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
  const deviceGroupsRef = useRef<DeviceGroupChoices | null>(null)
  const decisionBusyRef = useRef(false)
  const refreshSequenceRef = useRef(0)

  const refresh = useCallback(async () => {
    if (!enabled) return
    const sequence = ++refreshSequenceRef.current
    dispatch({ type: 'refresh_started' })
    try {
      const deviceGroups = await getDeviceGroupChoices()
      if (sequence !== refreshSequenceRef.current) return
      deviceGroupsRef.current = deviceGroups
      dispatch({ type: 'refresh_finished', deviceGroups })
    } catch (error) {
      if (sequence !== refreshSequenceRef.current) return
      dispatch({ type: 'refresh_failed', error: errorMessage(error) })
    }
  }, [enabled])

  useEffect(() => {
    if (!enabled) return
    const unsubscribe = daemonWs.subscribe(['device-trust', 'system'], () => void refresh())
    const reconnect = daemonWs.onReconnect(() => void refresh())
    void refresh()
    return () => {
      unsubscribe()
      reconnect()
    }
  }, [enabled, refresh])

  const refreshWhenVisible = useEffectEvent(() => {
    if (document.visibilityState === 'visible') void refresh()
  })
  const refreshOnFocus = useEffectEvent(() => {
    void refresh()
  })

  useEffect(() => {
    if (!enabled) return
    document.addEventListener('visibilitychange', refreshWhenVisible)
    window.addEventListener('focus', refreshOnFocus)
    return () => {
      document.removeEventListener('visibilitychange', refreshWhenVisible)
      window.removeEventListener('focus', refreshOnFocus)
    }
  }, [enabled])

  const choose = useCallback(
    async (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => {
      const deviceGroups = state.deviceGroups
      const issue = deviceGroups?.issues.find(candidate => candidate.issueId === issueId)
      if (
        !deviceGroups ||
        !issue?.choices.some(choice => choice.choiceId === choiceId) ||
        decisionBusyRef.current
      ) {
        return
      }
      if (deviceGroupsRef.current?.revision !== deviceGroups.revision) {
        await refresh()
        dispatch({ type: 'choice_outdated' })
        return
      }
      decisionBusyRef.current = true
      refreshSequenceRef.current += 1
      dispatch({
        type: 'choice_started',
        decision: { groups: deviceGroups, issueId, choiceId, outcome: 'submitting' },
      })
      try {
        const result = await chooseDeviceGroup(
          issueId,
          choiceId,
          deviceGroups.revision,
          confirmLocalRemoval
        )
        await refresh()
        dispatch({
          type: 'choice_finished',
          outcome: result.outcome,
          issueId,
        })
      } catch (error) {
        await refresh()
        dispatch({ type: 'choice_failed', error: errorMessage(error) })
      } finally {
        decisionBusyRef.current = false
      }
    },
    [refresh, state.deviceGroups]
  )

  const acknowledgeDecision = useCallback(async () => {
    dispatch({ type: 'acknowledged' })
    await refresh()
  }, [refresh])

  const value = useMemo(
    () => ({
      ...state,
      snapshot: state.deviceGroups?.deviceTrust ?? null,
      refresh,
      choose,
      acknowledgeDecision,
      cancelLocalConfirmation: () => dispatch({ type: 'confirmation_cancelled' }),
    }),
    [state, refresh, choose, acknowledgeDecision]
  )
  return <DeviceTrustContext value={value}>{children}</DeviceTrustContext>
}
