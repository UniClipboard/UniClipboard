import { useCallback, useEffect, useReducer, useRef } from 'react'
import { daemonClient } from '@/api/daemon/client'
import { signalLifecycleReady } from '@/api/daemon/lifecycle'
import { useEncryptionState } from '@/hooks/useDaemonEvents'
import { appBootstrapReducer, initialAppBootstrapState } from '@/lib/app-bootstrap-state'
import { DaemonBootstrapFailedError } from '@/lib/daemon-connection-info'
import { shouldSignalDaemonLifecycleReady } from '@/lib/daemon-lifecycle-ready'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands } from '@/lib/ipc'
import { reportError } from '@/observability/errors'
import { useGetEncryptionSessionStatusQuery } from '@/store/api'

const LOADING_WATCHDOG_MS = 12_000

export function useAppBootstrap(isSetupActive: boolean) {
  const [state, dispatch] = useReducer(appBootstrapReducer, initialAppBootstrapState)
  const bootstrapRetryingRef = useRef(false)
  const daemonLifecycleReadySignaledRef = useRef(false)

  useEffect(() => {
    if (isSetupActive) return

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
  }, [isSetupActive])

  const {
    data: encryptionData,
    isLoading: encryptionLoading,
    error: encryptionQueryError,
    refetch: refetchEncryption,
  } = useGetEncryptionSessionStatusQuery(undefined, {
    skip: isSetupActive || !state.daemonBootstrapReady,
  })

  const isInitialLoading =
    !isSetupActive &&
    state.encryptionOverride === null &&
    !state.bootEncryptionError &&
    !state.bootstrapFailure &&
    (encryptionLoading || !state.daemonBootstrapReady)
  useEffect(() => {
    if (!isInitialLoading) return
    const id = setTimeout(() => dispatch({ type: 'loadingTimedOut' }), LOADING_WATCHDOG_MS)
    return () => clearTimeout(id)
  }, [isInitialLoading])

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
    : (state.bootEncryptionError ??
      encryptionQueryErrorMessage ??
      (state.loadingTimedOut ? 'Timed out waiting for the background service.' : null))

  const retry = useCallback(() => {
    bootstrapRetryingRef.current = true
    dispatch({ type: 'retryStarted' })
    commands
      .restartDaemon()
      .then(() => connectDaemonWs())
      .then(() => {
        dispatch({ type: 'connectionReady' })
        return daemonClient.refreshSession()
      })
      .then(() => {
        void refetchEncryption()
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
      })
  }, [refetchEncryption])

  useEffect(() => {
    if (state.daemonBootstrapReady || state.bootstrapFailure) return

    let cancelled = false
    const id = setInterval(async () => {
      if (cancelled || bootstrapRetryingRef.current) return
      try {
        const failure = await commands.getDaemonBootstrapFailure()
        if (failure && !cancelled && !bootstrapRetryingRef.current) {
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
    encryptionLoading,
    encryptionError,
    resolvedEncryptionStatus,
    retry,
    setEncryptionStatus,
  }
}
