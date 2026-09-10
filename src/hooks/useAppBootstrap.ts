import { useCallback, useEffect, useReducer, useRef, useSyncExternalStore } from 'react'
import { daemonClient } from '@/api/daemon/client'
import { signalLifecycleReady } from '@/api/daemon/lifecycle'
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
  const subscribe = useCallback(
    (listener: () => void) => (state.daemonBootstrapReady ? () => {} : subscribeStartup(listener)),
    [state.daemonBootstrapReady]
  )
  const startupStatus = useSyncExternalStore(subscribe, getStartupSnapshot)
  const serviceReady = startupStatus?.service_ready ?? false

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
    skip: isSetupActive || !state.daemonBootstrapReady,
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
  const encryptionError = resolvedEncryptionStatus
    ? null
    : (state.bootEncryptionError ?? encryptionQueryErrorMessage)

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
      .then(() => checkEncryption(undefined, false).unwrap())
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
      !shouldSignalDaemonLifecycleReady(
        isSetupActive,
        state.daemonBootstrapReady,
        resolvedEncryptionStatus
      )
    ) {
      return
    }

    daemonLifecycleReadySignaledRef.current = true
    signalLifecycleReady().catch(error => {
      daemonLifecycleReadySignaledRef.current = false
      console.error('Failed to signal daemon lifecycle ready:', error)
    })
  }, [isSetupActive, resolvedEncryptionStatus, state.daemonBootstrapReady])

  const setEncryptionStatus = useCallback(
    (status: { initialized: boolean; session_ready: boolean }) =>
      dispatch({ type: 'encryptionStatusSet', status }),
    []
  )

  return {
    ...state,
    startupStatus,
    encryptionLoading,
    encryptionError,
    resolvedEncryptionStatus,
    retry,
    setEncryptionStatus,
  }
}
