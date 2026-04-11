import { useEffect, useSyncExternalStore } from 'react'
import {
  getSetupState,
  handleSpaceAccessCompleted,
  onSetupStateChanged,
  onSpaceAccessCompleted,
  type SetupState,
} from '@/api/setup'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { createLogger } from '@/lib/logger'

const log = createLogger('setup-realtime-store')

// ── Store-level realtime diagnostics ───────────────────────────────────────

/**
 * Record a store-level decision for setup realtime sync lifecycle events.
 *
 * Decisions:
 * - "started"    — sync initialization beginning
 * - "running"    — sync is active
 * - "skipped"    — action suppressed (already running, stale generation, etc.)
 * - "scheduled"  — retry timer scheduled after failure
 * - "failure"    — initialization error
 *
 * Security: never logs passphrase or encryption key material.
 */
function logStoreDecision(
  decision: 'started' | 'running' | 'skipped' | 'scheduled' | 'failure' | 'space_access_ignored',
  context: {
    reason?: string
    generation?: number
  } = {}
) {
  const { reason, generation } = context
  const parts: string[] = [`[setupRealtimeStore] ${decision}`]
  if (generation !== undefined) parts.push(`gen=${generation}`)
  if (reason) parts.push(`reason=${reason}`)

  log.debug(parts.join(' '))
}

type SetupRealtimeSnapshot = {
  setupState: SetupState | null
  sessionId: string | null
  hydrated: boolean
}

type SetupRealtimeStore = SetupRealtimeSnapshot & {
  syncSetupStateFromCommand: (nextState: SetupState) => void
}

const RETRY_DELAY_MS = 2000

let snapshot: SetupRealtimeSnapshot = {
  setupState: null,
  sessionId: null,
  hydrated: false,
}

const listeners = new Set<() => void>()
let stopListening: (() => void) | null = null
let stopListeningSpaceAccess: (() => void) | null = null
let startPromise: Promise<void> | null = null
let retryTimer: ReturnType<typeof setTimeout> | null = null
let syncGeneration = 0
let syncPhase: 'idle' | 'starting' | 'running' = 'idle'

function emitChange() {
  listeners.forEach(listener => listener())
}

function isSetupFlowActive(state: SetupState | null): boolean {
  return state !== null && state !== 'Welcome' && state !== 'Completed'
}

function clearRetryTimer() {
  if (!retryTimer) {
    return
  }

  clearTimeout(retryTimer)
  retryTimer = null
}

function updateSnapshot(nextState: SetupState, sessionId?: string | null) {
  snapshot = {
    setupState: nextState,
    sessionId: isSetupFlowActive(nextState) ? (sessionId ?? snapshot.sessionId) : null,
    hydrated: true,
  }
  emitChange()
}

function scheduleRetry() {
  if (retryTimer) {
    return
  }

  retryTimer = setTimeout(() => {
    retryTimer = null
    void ensureSetupRealtimeSync()
  }, RETRY_DELAY_MS)
}

export async function ensureSetupRealtimeSync(): Promise<void> {
  if (syncPhase === 'running') {
    logStoreDecision('skipped', { reason: 'already_running' })
    return
  }

  if (startPromise) {
    logStoreDecision('skipped', { reason: 'start_in_progress' })
    return startPromise
  }

  syncPhase = 'starting'
  const generation = ++syncGeneration
  logStoreDecision('started', { generation })

  startPromise = (async () => {
    try {
      clearRetryTimer()

      // Ensure daemon is connected before making API calls — the connection may not
      // have been established yet if this fires before AppContent calls connectDaemonWs().
      await connectDaemonWs()

      if (!snapshot.hydrated) {
        const initialState = await getSetupState()
        if (generation !== syncGeneration) {
          logStoreDecision('skipped', { reason: 'stale_generation_after_hydrate', generation })
          return
        }
        updateSnapshot(initialState, null)
      }

      const unlisten = await onSetupStateChanged(event => {
        if (generation !== syncGeneration) {
          logStoreDecision('skipped', { reason: 'stale_generation_in_state_changed', generation })
          return
        }

        updateSnapshot(event.state, event.sessionId)
      })

      if (generation !== syncGeneration) {
        logStoreDecision('skipped', { reason: 'stale_generation_after_state_listener', generation })
        unlisten()
        return
      }

      const unlistenSpaceAccess = await onSpaceAccessCompleted(async event => {
        if (generation !== syncGeneration) {
          logStoreDecision('skipped', { reason: 'stale_generation_in_space_access', generation })
          return
        }

        // Skip if setup is already completed (sponsor role — this event fires on both
        // sponsor and joiner sides, but only the joiner needs to finalize setup here).
        if (snapshot.setupState === 'Completed') {
          logStoreDecision('space_access_ignored', { reason: 'setup_already_completed' })
          return
        }

        try {
          const newState = await handleSpaceAccessCompleted()
          updateSnapshot(newState, event.sessionId)
        } catch (error) {
          logStoreDecision('failure', { reason: 'handleSpaceAccessCompleted_error' })
          log.error({ err: error }, 'Failed to handle space access completed')
        }
      })

      if (generation !== syncGeneration) {
        logStoreDecision('skipped', {
          reason: 'stale_generation_after_space_access_listener',
          generation,
        })
        unlisten()
        unlistenSpaceAccess()
        return
      }

      stopListening = unlisten
      stopListeningSpaceAccess = unlistenSpaceAccess
      syncPhase = 'running'
      logStoreDecision('running', { generation })
    } catch (error) {
      if (generation !== syncGeneration) {
        return
      }

      logStoreDecision('failure', { reason: 'initialization_error', generation })
      log.error({ err: error }, 'Failed to initialize setup realtime store')
      syncPhase = 'idle'
      scheduleRetry()
      logStoreDecision('scheduled', { reason: 'retry_after_init_failure', generation })
    } finally {
      if (syncPhase !== 'running') {
        startPromise = null
      }
    }
  })()

  return startPromise
}

export function syncSetupStateFromCommand(nextState: SetupState) {
  updateSnapshot(nextState)
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot(): SetupRealtimeSnapshot {
  return snapshot
}

export function useSetupRealtimeStore(): SetupRealtimeStore {
  const currentSnapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)

  useEffect(() => {
    void ensureSetupRealtimeSync()
  }, [])

  return {
    ...currentSnapshot,
    syncSetupStateFromCommand,
  }
}

export function resetSetupRealtimeStoreForTests() {
  syncGeneration += 1
  syncPhase = 'idle'
  startPromise = null
  clearRetryTimer()

  if (stopListening) {
    stopListening()
    stopListening = null
  }

  if (stopListeningSpaceAccess) {
    stopListeningSpaceAccess()
    stopListeningSpaceAccess = null
  }

  snapshot = {
    setupState: null,
    sessionId: null,
    hydrated: false,
  }

  emitChange()
}
