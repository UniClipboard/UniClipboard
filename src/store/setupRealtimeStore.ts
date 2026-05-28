import { useEffect, useSyncExternalStore } from 'react'
import { getSetupState, type SetupStateResponse, SetupV2Error } from '@/api/daemon/setupV2'
import {
  onSetupInvitationIssued,
  onSetupInvitationRevoked,
  onSetupPairingCompleted,
  type SetupInvitationIssuedEvent,
  type SetupInvitationRevokedEvent,
  type SetupPairingCompletedEvent,
} from '@/api/setupEvents'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { createLogger } from '@/lib/logger'

const log = createLogger('setup-realtime-store')

/**
 * Coarse setup gate state derived from `GET /v2/setup/state` and the three
 * setup ws events. Anything finer-grained (which screen the user is on
 * inside `entry` mode, in-flight requests, transient errors) is held in
 * `useSetupFlow` page-local state, not here.
 */
export type SetupFlow =
  /** Initial fetch in progress; setup gate stays active. */
  | { kind: 'loading' }
  /** No space initialised yet — show the entry / initialise / redeem screens. */
  | { kind: 'entry' }
  /** Sponsor has issued an invitation; resume the show-code screen on launch. */
  | { kind: 'invitation_pending'; code: string; expiresAtMs: number }
  /** Setup has completed and there is no in-flight invitation; gate is closed. */
  | { kind: 'completed'; deviceName: string | null }

interface Snapshot {
  flow: SetupFlow
  hydrated: boolean
}

const RETRY_DELAY_MS = 2000

let snapshot: Snapshot = {
  flow: { kind: 'loading' },
  hydrated: false,
}

const listeners = new Set<() => void>()
let startPromise: Promise<void> | null = null
let retryTimer: ReturnType<typeof setTimeout> | null = null
let syncGeneration = 0
let syncPhase: 'idle' | 'starting' | 'running' = 'idle'

function emitChange() {
  for (const listener of listeners) listener()
}

function clearRetryTimer() {
  if (retryTimer === null) return
  clearTimeout(retryTimer)
  retryTimer = null
}

function flowFromState(state: SetupStateResponse): SetupFlow {
  if (state.currentInvitation) {
    return {
      kind: 'invitation_pending',
      code: state.currentInvitation.code,
      expiresAtMs: state.currentInvitation.expiresAtMs,
    }
  }
  if (state.hasCompleted) {
    return { kind: 'completed', deviceName: state.deviceName }
  }
  return { kind: 'entry' }
}

function update(flow: SetupFlow, hydrated = true) {
  snapshot = { flow, hydrated }
  emitChange()
}

function applyInvitationIssued(event: SetupInvitationIssuedEvent) {
  // Sponsor issued a new invitation — switch to the show-code screen even if
  // we previously thought we were in `completed` state.
  update({ kind: 'invitation_pending', code: event.code, expiresAtMs: event.expiresAtMs })
}

function applyInvitationRevoked(_event: SetupInvitationRevokedEvent) {
  // Invitation cancelled or expired — refresh from the server to discover
  // whether `hasCompleted` is true (sponsor stays in `completed`) or false
  // (this device hasn't initialised yet, drop back to entry).
  void refreshFromServer()
}

function applyPairingCompleted(_event: SetupPairingCompletedEvent) {
  // Either side finished a handshake. The sponsor's invitation is now
  // consumed; `hasCompleted` may have flipped on the joiner. Refresh the
  // authoritative state from the server.
  void refreshFromServer()
}

async function refreshFromServer() {
  const generation = syncGeneration
  try {
    const next = await getSetupState()
    if (generation !== syncGeneration) return
    update(flowFromState(next))
  } catch (err) {
    if (err instanceof SetupV2Error) {
      log.warn({ kind: err.kind, raw: err.raw }, 'failed to refresh setup state')
    } else {
      log.warn({ err }, 'failed to refresh setup state')
    }
  }
}

function scheduleRetry() {
  if (retryTimer !== null) return
  retryTimer = setTimeout(() => {
    retryTimer = null
    void ensureSetupRealtimeSync()
  }, RETRY_DELAY_MS)
}

export async function ensureSetupRealtimeSync(): Promise<void> {
  if (syncPhase === 'running') return
  if (startPromise) return startPromise

  syncPhase = 'starting'
  const generation = ++syncGeneration

  startPromise = (async () => {
    try {
      clearRetryTimer()
      await connectDaemonWs()

      const initial = await getSetupState()
      if (generation !== syncGeneration) return
      update(flowFromState(initial))

      const offIssued = await onSetupInvitationIssued(applyInvitationIssued)
      if (generation !== syncGeneration) {
        offIssued()
        return
      }
      const offRevoked = await onSetupInvitationRevoked(applyInvitationRevoked)
      if (generation !== syncGeneration) {
        offIssued()
        offRevoked()
        return
      }
      const offCompleted = await onSetupPairingCompleted(applyPairingCompleted)
      if (generation !== syncGeneration) {
        offIssued()
        offRevoked()
        offCompleted()
        return
      }

      // Stash unlisten handles in the closure-rooted symbols so they survive
      // the lifetime of the singleton store; we never tear them down.
      void offIssued
      void offRevoked
      void offCompleted
      syncPhase = 'running'
    } catch (err) {
      if (generation !== syncGeneration) return
      log.error({ err }, 'failed to initialise setup realtime sync')
      syncPhase = 'idle'
      scheduleRetry()
    } finally {
      if (syncPhase !== 'running') {
        startPromise = null
      }
    }
  })()

  return startPromise
}

/**
 * Imperatively merge a fresh `SetupStateResponse` (from a REST call the
 * page just made) into the store. Use this after `initializeSpace`,
 * `redeemInvitation`, `cancelInvitation`, or `resetSetup` so the UI
 * reflects the change before the (possibly slower) ws roundtrip lands.
 */
export function applyServerSetupState(state: SetupStateResponse) {
  update(flowFromState(state))
}

/** Force a `GET /v2/setup/state` refresh from page code. */
export async function refreshSetupState(): Promise<void> {
  await refreshFromServer()
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot(): Snapshot {
  return snapshot
}

export function useSetupRealtimeStore(): Snapshot {
  const current = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)

  useEffect(() => {
    void ensureSetupRealtimeSync()
  }, [])

  return current
}
