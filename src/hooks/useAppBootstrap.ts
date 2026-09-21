import { useCallback, useEffect, useReducer, useRef, useState, useSyncExternalStore } from 'react'
import { daemonClient } from '@/api/daemon/client'
import { getProfileRecovery, type ProfileRecoveryResponse } from '@/api/daemon/encryption'
import { getLifecycleStatus, signalLifecycleReady } from '@/api/daemon/lifecycle'
import { useEncryptionState } from '@/hooks/useDaemonEvents'
import { appBootstrapReducer, initialAppBootstrapState } from '@/lib/app-bootstrap-state'
import { DaemonBootstrapFailedError } from '@/lib/daemon-connection-info'
import { shouldSignalDaemonLifecycleReady } from '@/lib/daemon-lifecycle-ready'
import { getStartupSnapshot, subscribeStartup } from '@/lib/daemon-startup-progress'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands } from '@/lib/ipc'
import { reportError } from '@/observability/errors'
import {
  useGetEncryptionSessionStatusQuery,
  useLazyGetEncryptionSessionStatusQuery,
} from '@/store/api'

export function useAppBootstrap(isSetupActive: boolean) {
  const [state, dispatch] = useReducer(appBootstrapReducer, initialAppBootstrapState)
  const bootstrapRetryingRef = useRef(false)
  const daemonLifecycleReadySignaledRef = useRef(false)
  const [spaceReadiness, setSpaceReadiness] = useState<
    'checking' | 'recoveringMembership' | 'ready'
  >('checking')
  const [profileRecovery, setProfileRecovery] = useState<ProfileRecoveryResponse | null>(null)
  const [profileRecoveryLoading, setProfileRecoveryLoading] = useState(false)
  const [profileRecoveryError, setProfileRecoveryError] = useState<string | null>(null)
  const subscribe = useCallback(
    (listener: () => void) => (state.daemonBootstrapReady ? () => {} : subscribeStartup(listener)),
    [state.daemonBootstrapReady]
  )
  const startupStatus = useSyncExternalStore(subscribe, getStartupSnapshot)
  const serviceReady = startupStatus?.service_ready ?? false

  useEffect(() => {
    if (!state.daemonBootstrapReady) {
      setProfileRecovery(null)
      setProfileRecoveryLoading(false)
      setProfileRecoveryError(null)
      return
    }

    let cancelled = false
    setProfileRecoveryLoading(true)
    setProfileRecoveryError(null)
    getProfileRecovery()
      .then(status => {
        if (!cancelled) setProfileRecovery(status)
      })
      .catch(error => {
        if (!cancelled) {
          setProfileRecoveryError(error instanceof Error ? error.message : String(error))
        }
      })
      .finally(() => {
        if (!cancelled) setProfileRecoveryLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [state.daemonBootstrapReady])

  useEffect(() => {
    if (bootstrapRetryingRef.current) return

    let cancelled = false
    connectDaemonWs()
      .then(() => {
        if (!cancelled) dispatch({ type: 'connectionReady' })
      })
      .catch(error => {
        if (cancelled) return
        dispatch({
          type: 'connectionFailed',
          error: error instanceof Error ? error.message : String(error),
          failure: error instanceof DaemonBootstrapFailedError ? error.failure : null,
        })
      })

    return () => {
      cancelled = true
    }
  }, [serviceReady])

  const {
    data: encryptionData,
    isLoading: encryptionLoading,
    error: encryptionQueryError,
  } = useGetEncryptionSessionStatusQuery(undefined, {
    skip:
      isSetupActive ||
      !state.daemonBootstrapReady ||
      profileRecoveryLoading ||
      !profileRecovery?.backgroundReady,
  })
  const [checkEncryption] = useLazyGetEncryptionSessionStatusQuery()

  useEncryptionState(
    () => dispatch({ type: 'encryptionReady' }),
    () => dispatch({ type: 'encryptionNotReady' })
  )

  const encryptionQueryErrorMessage = encryptionQueryError
    ? typeof encryptionQueryError === 'object' && 'message' in encryptionQueryError
      ? String(encryptionQueryError.message)
      : 'Failed to check encryption status'
    : null
  const resolvedEncryptionStatus = state.encryptionOverride ?? encryptionData ?? null
  const encryptionInitialized = resolvedEncryptionStatus?.initialized === true
  const encryptionSessionReady = resolvedEncryptionStatus?.session_ready === true
  const recoveryRequired = profileRecovery ? !profileRecovery.backgroundReady : false
  const encryptionError = recoveryRequired
    ? null
    : resolvedEncryptionStatus
      ? null
      : (state.bootEncryptionError ?? encryptionQueryErrorMessage)

  useEffect(() => {
    if (
      isSetupActive ||
      !state.daemonBootstrapReady ||
      recoveryRequired ||
      !encryptionInitialized ||
      !encryptionSessionReady
    ) {
      setSpaceReadiness('checking')
      return
    }

    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | null = null
    const refresh = async () => {
      try {
        const status = await getLifecycleStatus()
        if (cancelled) return
        if (status.state === 'Ready') {
          setSpaceReadiness('ready')
          return
        }
        setSpaceReadiness(
          status.pendingReason === 'membership_recovery' ? 'recoveringMembership' : 'checking'
        )
      } catch {
        if (cancelled) return
        setSpaceReadiness('checking')
      }
      timer = setTimeout(refresh, 1_000)
    }
    void refresh()

    return () => {
      cancelled = true
      if (timer !== null) clearTimeout(timer)
    }
  }, [
    encryptionInitialized,
    encryptionSessionReady,
    isSetupActive,
    recoveryRequired,
    state.daemonBootstrapReady,
  ])

  const retry = useCallback(() => {
    if (bootstrapRetryingRef.current) return
    bootstrapRetryingRef.current = true
    dispatch({ type: 'retryStarted' })
    commands
      .restartDaemon()
      .then(() => connectDaemonWs())
      .then(() => {
        return daemonClient.refreshSession()
      })
      .then(() => getProfileRecovery())
      .then(status => {
        setProfileRecovery(status)
        if (!status.backgroundReady) return null
        return checkEncryption(undefined, false).unwrap()
      })
      .then(() => {
        dispatch({ type: 'connectionReady' })
      })
      .catch(error => {
        dispatch({
          type: 'connectionFailed',
          error: error instanceof Error ? error.message : String(error),
          failure: error instanceof DaemonBootstrapFailedError ? error.failure : null,
        })
      })
      .finally(() => {
        bootstrapRetryingRef.current = false
        dispatch({ type: 'retryFinished' })
      })
  }, [checkEncryption])

  useEffect(() => {
    if (state.daemonBootstrapReady || state.bootstrapFailure) return

    let cancelled = false
    const id = setInterval(async () => {
      if (cancelled || bootstrapRetryingRef.current) return
      try {
        const failure = await commands.getDaemonBootstrapFailure()
        const active = getStartupSnapshot()
        if (
          failure &&
          !cancelled &&
          !bootstrapRetryingRef.current &&
          (!active ||
            active.service_failed ||
            ['failed', 'interrupted'].includes(active.progress.state) ||
            failure.kind === 'versionTooOld')
        ) {
          dispatch({ type: 'bootstrapFailed', failure })
          reportError(new Error(`Daemon bootstrap failed: ${failure.kind}`), {
            kind: failure.kind,
            detail: failure.detail,
            observedVersion: failure.observedVersion,
            expectedVersion: failure.expectedVersion,
          })
        }
      } catch {
        // Best-effort; the Tauri command itself failing is non-fatal.
      }
    }, 1_000)

    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [state.daemonBootstrapReady, state.bootstrapFailure])

  useEffect(() => {
    if (
      daemonLifecycleReadySignaledRef.current ||
      recoveryRequired ||
      !shouldSignalDaemonLifecycleReady(
        isSetupActive,
        state.daemonBootstrapReady,
        resolvedEncryptionStatus,
        spaceReadiness
      )
    ) {
      return
    }

    daemonLifecycleReadySignaledRef.current = true
    signalLifecycleReady().catch(error => {
      daemonLifecycleReadySignaledRef.current = false
      console.error('Failed to signal daemon lifecycle ready:', error)
    })
  }, [
    isSetupActive,
    recoveryRequired,
    resolvedEncryptionStatus,
    spaceReadiness,
    state.daemonBootstrapReady,
  ])

  const setEncryptionStatus = useCallback(
    (status: { initialized: boolean; session_ready: boolean }) =>
      dispatch({ type: 'encryptionStatusSet', status }),
    []
  )

  return {
    ...state,
    startupStatus,
    profileRecovery,
    profileRecoveryLoading,
    profileRecoveryError,
    encryptionLoading,
    encryptionError,
    resolvedEncryptionStatus,
    spaceReadiness,
    retry,
    setEncryptionStatus,
  }
}
