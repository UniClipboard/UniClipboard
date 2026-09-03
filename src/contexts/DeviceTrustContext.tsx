import { useCallback, useEffect, useMemo, useReducer, useRef, type ReactNode } from 'react'
import {
  chooseDeviceGroup,
  getDeviceGroupChoices,
  type DeviceGroupChoiceOutcome,
  type DeviceGroupChoices,
} from '@/api/daemon/device-trust'
import { DeviceTrustContext } from '@/contexts/device-trust-context'
import { daemonWs } from '@/lib/daemon-ws'

interface DeviceTrustState {
  deviceGroups: DeviceGroupChoices | null
  loading: boolean
  decisionBusy: boolean
  decisionError: string | null
  localRemovalConfirmationIssueId: string | null
}

type DeviceTrustStateAction =
  | { type: 'refresh_started' }
  | { type: 'refresh_finished'; deviceGroups: DeviceGroupChoices }
  | { type: 'refresh_failed'; error: string }
  | { type: 'choice_started' }
  | {
      type: 'choice_finished'
      deviceGroups: DeviceGroupChoices
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
}

function issueStillCurrent(deviceGroups: DeviceGroupChoices, issueId: string | null): boolean {
  return issueId !== null && deviceGroups.issues.some(issue => issue.issueId === issueId)
}

function stateReducer(state: DeviceTrustState, action: DeviceTrustStateAction): DeviceTrustState {
  switch (action.type) {
    case 'refresh_started':
      return { ...state, loading: true }
    case 'refresh_finished':
      return {
        ...state,
        deviceGroups: action.deviceGroups,
        loading: false,
        decisionError: null,
        localRemovalConfirmationIssueId: issueStillCurrent(
          action.deviceGroups,
          state.localRemovalConfirmationIssueId
        )
          ? state.localRemovalConfirmationIssueId
          : null,
      }
    case 'refresh_failed':
      return { ...state, loading: false, decisionError: action.error }
    case 'choice_started':
      return { ...state, decisionBusy: true, decisionError: null }
    case 'choice_finished':
      return {
        ...state,
        deviceGroups: action.deviceGroups,
        loading: false,
        decisionBusy: false,
        decisionError:
          action.outcome === 'state_changed'
            ? 'device_state_changed'
            : action.outcome === 'pending'
              ? 'choice_pending'
              : action.outcome === 're_pairing_required'
                ? 're_pairing_required'
                : null,
        localRemovalConfirmationIssueId:
          action.outcome === 'local_device_confirmation_required' &&
          issueStillCurrent(action.deviceGroups, action.issueId)
            ? action.issueId
            : null,
      }
    case 'choice_failed':
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
    void refresh()
    return daemonWs.subscribe(['device-trust', 'system'], () => void refresh())
  }, [enabled, refresh])

  useEffect(() => {
    if (!enabled) return
    const onVisible = () => {
      if (document.visibilityState === 'visible') void refresh()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => document.removeEventListener('visibilitychange', onVisible)
  }, [enabled, refresh])

  const choose = useCallback(
    async (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => {
      const deviceGroups = deviceGroupsRef.current
      const issue = deviceGroups?.issues.find(candidate => candidate.issueId === issueId)
      if (
        !deviceGroups ||
        !issue?.choices.some(choice => choice.choiceId === choiceId) ||
        decisionBusyRef.current
      ) {
        return
      }
      decisionBusyRef.current = true
      refreshSequenceRef.current += 1
      dispatch({ type: 'choice_started' })
      try {
        const result = await chooseDeviceGroup(
          issueId,
          choiceId,
          deviceGroups.revision,
          confirmLocalRemoval
        )
        const sequence = ++refreshSequenceRef.current
        const latest = await getDeviceGroupChoices()
        if (sequence !== refreshSequenceRef.current) return
        deviceGroupsRef.current = latest
        dispatch({
          type: 'choice_finished',
          deviceGroups: latest,
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
    [refresh]
  )

  const value = useMemo(
    () => ({
      ...state,
      snapshot: state.deviceGroups?.deviceTrust ?? null,
      refresh,
      choose,
    }),
    [state, refresh, choose]
  )
  return <DeviceTrustContext value={value}>{children}</DeviceTrustContext>
}
