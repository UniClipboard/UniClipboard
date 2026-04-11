import { useCallback, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import type { SetupState } from '@/api/daemon/setup'
import { createLogger } from '@/lib/logger'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'

const log = createLogger('use-setup-flow')

export interface StepInfo {
  total: number
  current: number
}

export interface UseSetupFlowReturn {
  setupState: SetupState | null
  hydrated: boolean
  stepInfo: StepInfo | null
  direction: 'forward' | 'backward'
  loading: boolean
  runAction: (action: () => Promise<SetupState>) => Promise<void>
  selectedPeerId: string | null
  setSelectedPeerId: (peerId: string | null) => void
}

// ── Private helpers (extracted from SetupPage.tsx) ───────────────────────────

function getStateOrdinal(state: SetupState | null): number {
  if (!state) return -1
  if (state === 'Welcome') return 0
  if (state === 'Completed') return 99
  if (typeof state === 'object') {
    if ('CreateSpaceInputPassphrase' in state) return 1
    if ('ProcessingCreateSpace' in state) return 2
    if ('JoinSpaceSelectDevice' in state) return 1
    if ('JoinSpaceConfirmPeer' in state) return 2
    if ('JoinSpaceInputPassphrase' in state) return 3
    if ('ProcessingJoinSpace' in state) return 4
  }
  return -1
}

function getStepInfo(
  state: SetupState | null,
  prevState?: SetupState | null
): { total: number; current: number } | null {
  if (!state || state === 'Welcome') return null
  if (state === 'Completed') return null
  if (typeof state === 'object') {
    if ('CreateSpaceInputPassphrase' in state) return { total: 3, current: 0 }
    if ('ProcessingCreateSpace' in state) return { total: 3, current: 1 }
    if ('JoinSpaceSelectDevice' in state) return { total: 4, current: 0 }
    if ('JoinSpaceConfirmPeer' in state) return { total: 4, current: 1 }
    if ('JoinSpaceInputPassphrase' in state) return { total: 4, current: 2 }
    if ('ProcessingJoinSpace' in state) {
      const isConnectingPhase =
        prevState && typeof prevState === 'object' && 'JoinSpaceSelectDevice' in prevState
      return { total: 4, current: isConnectingPhase ? 0 : 3 }
    }
  }
  return null
}

// ── Hook ───────────────────────────────────────────────────────────────────

export function useSetupFlow(): UseSetupFlowReturn {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.page' })
  const { setupState, hydrated, syncSetupStateFromCommand } = useSetupRealtimeStore()
  const [loading, setLoading] = useState(false)
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null)
  const prevStateRef = useRef<SetupState | null>(null)

  const direction = useMemo(() => {
    return getStateOrdinal(setupState) >= getStateOrdinal(prevStateRef.current)
      ? 'forward'
      : 'backward'
  }, [setupState])

  // Update prevStateRef after render so getStepInfo gets the correct previous state
  useMemo(() => {
    prevStateRef.current = setupState
  }, [setupState])

  // prevStateRef.current is read during render (before the useMemo updates it),
  // so it still holds the previous state — exactly what getStepInfo needs.
  const stepInfo = useMemo(() => getStepInfo(setupState, prevStateRef.current), [setupState])

  const runAction = useCallback(
    async (action: () => Promise<SetupState>) => {
      setLoading(true)
      try {
        const newState = await action()
        syncSetupStateFromCommand(newState)
      } catch (error) {
        log.error({ err: error }, 'Failed to dispatch event')
        toast.error(t('errors.operationFailed'))
      } finally {
        setLoading(false)
      }
    },
    [syncSetupStateFromCommand, t]
  )

  return {
    setupState,
    hydrated,
    stepInfo,
    direction,
    loading,
    runAction,
    selectedPeerId,
    setSelectedPeerId,
  }
}
