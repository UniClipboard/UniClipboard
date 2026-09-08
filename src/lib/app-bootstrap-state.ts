import type { EncryptionStatusView } from '@/lib/daemon-lifecycle-ready'
import type { DaemonBootstrapFailure } from '@/lib/ipc'

export type AppBootstrapState = {
  encryptionOverride: EncryptionStatusView | null
  bootEncryptionError: string | null
  bootstrapFailure: DaemonBootstrapFailure | null
  daemonBootstrapReady: boolean
  loadingTimedOut: boolean
}

type AppBootstrapAction =
  | { type: 'connectionReady' }
  | { type: 'connectionFailed'; error: string; failure: DaemonBootstrapFailure | null }
  | { type: 'retryStarted' }
  | { type: 'bootstrapFailed'; failure: DaemonBootstrapFailure }
  | { type: 'encryptionReady' }
  | { type: 'encryptionNotReady' }
  | { type: 'encryptionStatusSet'; status: EncryptionStatusView }
  | { type: 'loadingTimedOut' }

export const initialAppBootstrapState: AppBootstrapState = {
  encryptionOverride: null,
  bootEncryptionError: null,
  bootstrapFailure: null,
  daemonBootstrapReady: false,
  loadingTimedOut: false,
}

export function appBootstrapReducer(
  state: AppBootstrapState,
  action: AppBootstrapAction
): AppBootstrapState {
  switch (action.type) {
    case 'connectionReady':
      return {
        ...state,
        bootEncryptionError: null,
        bootstrapFailure: null,
        daemonBootstrapReady: true,
      }
    case 'connectionFailed':
      return {
        ...state,
        bootEncryptionError: action.error,
        bootstrapFailure: action.failure,
      }
    case 'retryStarted':
      return {
        ...state,
        bootEncryptionError: null,
        bootstrapFailure: null,
        loadingTimedOut: false,
      }
    case 'bootstrapFailed':
      return {
        ...state,
        bootEncryptionError: action.failure.detail || 'The background service failed to start.',
        bootstrapFailure: action.failure,
      }
    case 'encryptionReady':
      return {
        ...state,
        encryptionOverride: state.encryptionOverride
          ? { ...state.encryptionOverride, session_ready: true }
          : { initialized: true, session_ready: true },
      }
    case 'encryptionNotReady':
      return {
        ...state,
        encryptionOverride: state.encryptionOverride
          ? { ...state.encryptionOverride, session_ready: false }
          : { initialized: true, session_ready: false },
      }
    case 'encryptionStatusSet':
      return { ...state, encryptionOverride: action.status }
    case 'loadingTimedOut':
      return { ...state, loadingTimedOut: true }
  }
}
