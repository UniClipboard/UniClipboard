/**
 * Setup API — stable facade layer delegating to daemon HTTP endpoints.
 *
 * This module is the public API surface for setup functionality.
 * All calls go through daemon HTTP; event listeners use the daemon
 * realtime WebSocket bridge.
 */

import {
  getSetupState as daemonGetSetupState,
  startNewSpace as daemonStartNewSpace,
  startJoinSpace as daemonStartJoinSpace,
  selectJoinPeer as daemonSelectJoinPeer,
  submitPassphrase as daemonSubmitPassphrase,
  verifyPassphrase as daemonVerifyPassphrase,
  confirmPeerTrust as daemonConfirmPeerTrust,
  cancelSetup as daemonCancelSetup,
  completeSpaceAccess as daemonCompleteSpaceAccess,
} from '@/api/daemon/setup'
import type {
  SetupState,
  SetupStateChangedEvent,
  SpaceAccessCompletedEvent,
} from '@/api/daemon/setup'
import { onDaemonRealtimeEvent } from '@/api/realtime'
import { createLogger } from '@/lib/logger'

const log = createLogger('setup')

// ── Setup realtime diagnostics ──────────────────────────────────────────────

/**
 * Record a setup realtime routing decision for diagnosability.
 *
 * Decisions:
 * - "applied"     — event was passed to the caller's callback
 * - "dropped"     — event was suppressed (dedupe, missing session id, etc.)
 * - "session_switched" — session id changed, dedupe key set cleared
 *
 * Security: never logs passphrase, short_code, or fingerprint fields.
 */
function logSetupRouting(
  decision: 'applied' | 'dropped' | 'session_switched',
  context: {
    eventType: string
    sessionId?: string | null
    activeSessionId?: string | null
    state?: string
    reason?: string
  }
) {
  const { eventType, sessionId, activeSessionId, state, reason } = context
  const parts: string[] = [`[setup.ts] ${decision}`, `event_type=${eventType}`]
  if (sessionId !== undefined) parts.push(`session_id=${sessionId ?? 'null'}`)
  if (activeSessionId !== undefined) parts.push(`active_session_id=${activeSessionId ?? 'null'}`)
  if (state !== undefined) parts.push(`state=${state}`)
  if (reason) parts.push(`reason=${reason}`)

  log.debug(parts.join(' '))
}

// Types are defined in the daemon module to avoid circular imports.
// Re-export them so consumers can import from either location.
export type {
  SetupError,
  SetupState,
  SetupStateChangedEvent,
  SetupStateResponse,
  SpaceAccessCompletedEvent,
} from '@/api/daemon/setup'

/**
 * Get current setup state
 * 获取当前设置流程状态
 */
export async function getSetupState(): Promise<SetupState> {
  return daemonGetSetupState()
}

/**
 * Start new space flow
 * 启动新空间流程
 */
export async function startNewSpace(): Promise<SetupState> {
  return daemonStartNewSpace()
}

/**
 * Start join space flow
 * 启动加入空间流程
 */
export async function startJoinSpace(): Promise<SetupState> {
  return daemonStartJoinSpace()
}

/**
 * Select peer device to join
 * 选择加入空间的设备
 */
export async function selectJoinPeer(peerId: string): Promise<SetupState> {
  return daemonSelectJoinPeer(peerId)
}

/**
 * Submit passphrase for new space
 * 提交新空间口令
 */
export async function submitPassphrase(
  passphrase1: string,
  passphrase2: string
): Promise<SetupState> {
  // Local mismatch check is handled inside daemonSubmitPassphrase.
  return daemonSubmitPassphrase(passphrase1, passphrase2)
}

/**
 * Verify passphrase for join space
 * 校验加入空间口令
 */
export async function verifyPassphrase(passphrase: string): Promise<SetupState> {
  return daemonVerifyPassphrase(passphrase)
}

/**
 * Confirm trust for the selected peer device
 * 确认选中设备的可信度
 */
export async function confirmPeerTrust(): Promise<SetupState> {
  return daemonConfirmPeerTrust()
}

/**
 * Cancel setup flow
 * 取消设置流程
 */
export async function cancelSetup(): Promise<SetupState> {
  return daemonCancelSetup()
}

/**
 * Called by the frontend when the daemon emits `setup.spaceAccessCompleted` via
 * the WebSocket bridge. This bridges the gap between the daemon's space access
 * orchestrator completing and the app's setup orchestrator transitioning to
 * `Completed`.
 */
export async function handleSpaceAccessCompleted(): Promise<SetupState> {
  return daemonCompleteSpaceAccess()
}

/**
 * Listen for space access completion events from the daemon.
 * This is used to transition the setup state machine from `ProcessingJoinSpace`
 * to `Completed` when the daemon's space access flow completes.
 */
export async function onSpaceAccessCompleted(
  callback: (event: SpaceAccessCompletedEvent) => void
): Promise<() => void> {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== 'setup' || event.type !== 'setup.spaceAccessCompleted') {
      return
    }
    callback(event.payload as SpaceAccessCompletedEvent)
  })
}

/**
 * Listen setup state changes with session idempotency.
 */
export async function onSetupStateChanged(
  callback: (event: SetupStateChangedEvent) => void
): Promise<() => void> {
  let activeSessionId: string | null = null
  const seenEventKeys = new Set<string>()

  return onDaemonRealtimeEvent(event => {
    if (event.topic !== 'setup' || event.type !== 'setup.stateChanged') {
      return
    }

    const payload = event.payload as Omit<SetupStateChangedEvent, 'ts' | 'source'>
    const enrichedEvent: SetupStateChangedEvent = {
      ...payload,
      source: 'realtime',
      ts: event.ts,
    }

    if (!enrichedEvent.sessionId) {
      logSetupRouting('dropped', { eventType: 'setup.stateChanged', reason: 'missing_session_id' })
      return
    }

    if (activeSessionId !== enrichedEvent.sessionId) {
      logSetupRouting('session_switched', {
        eventType: 'setup.stateChanged',
        sessionId: enrichedEvent.sessionId,
        activeSessionId,
      })
      activeSessionId = enrichedEvent.sessionId
      seenEventKeys.clear()
    }

    const stateKey =
      typeof enrichedEvent.state === 'string'
        ? enrichedEvent.state
        : (Object.keys(enrichedEvent.state as object)[0] ?? 'unknown')
    const dedupeKey = `${enrichedEvent.sessionId}:${JSON.stringify(enrichedEvent.state)}:${enrichedEvent.ts}`
    if (seenEventKeys.has(dedupeKey)) {
      logSetupRouting('dropped', {
        eventType: 'setup.stateChanged',
        sessionId: enrichedEvent.sessionId,
        state: stateKey,
        reason: 'duplicate_state_event',
      })
      return
    }
    seenEventKeys.add(dedupeKey)

    logSetupRouting('applied', {
      eventType: 'setup.stateChanged',
      sessionId: enrichedEvent.sessionId,
      state: stateKey,
    })
    callback(enrichedEvent)

    if (enrichedEvent.state === 'Completed') {
      activeSessionId = null
      seenEventKeys.clear()
    }
  })
}
