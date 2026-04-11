/**
 * React hooks for subscribing to daemon WebSocket events.
 *
 * These hooks wrap `daemonWs.subscribe()` with React's useEffect lifecycle:
 * they subscribe on mount, re-subscribe automatically when the daemon reconnects
 * (daemonWs maintains active topics across reconnections), and unsubscribe on unmount.
 *
 * All hooks require `daemonWs` to be connected first (call `daemonWs.connect(wsUrl)`).
 */

import { useEffect, useRef } from 'react'
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import { daemonWs } from '@/lib/daemon-ws'
import type { DaemonWsEvent } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-daemon-events')

// ── Routing diagnostics ─────────────────────────────────────────

/**
 * Record a routing decision for a pairing/setup event at the shared hook boundary.
 *
 * Decisions:
 * - "routed"   — event was dispatched to the matching callback
 * - "ignored"  — event was silently skipped (no callback registered, session mismatch, etc.)
 * - "unsupported" — event type / payload shape not recognised
 *
 * Security: never logs code, fingerprint, or passphrase fields.
 */
function logPairingRouting(
  decision: 'routed' | 'ignored' | 'unsupported',
  context: {
    eventType: string
    sessionId?: string
    kind?: string
    state?: string
    reason?: string
  }
) {
  const { eventType, sessionId, kind, state, reason } = context
  const parts: string[] = [`[useDaemonEvents] ${decision}`, `event_type=${eventType}`]
  if (sessionId) parts.push(`session_id=${sessionId}`)
  if (kind !== undefined) parts.push(`kind=${kind}`)
  if (state !== undefined) parts.push(`state=${state}`)
  if (reason) parts.push(`reason=${reason}`)

  log.debug(parts.join(' '))
}

// ── Payload types for daemon WS events ─────────────────────────

/** Payload for `clipboard.new_content` events. */
export interface ClipboardNewContentPayload {
  entry: ClipboardEntryDto
  origin: 'local' | 'remote'
}

/** Payload for `encryption.session_ready` events. */
export interface EncryptionSessionReadyPayload {
  sessionId: string
}

/** Payload for `encryption.session_failed` events (daemon never emits this — reserved for future use). */
export interface EncryptionSessionFailedPayload {
  reason?: string
}

/** Payload for `setup.spaceAccessCompleted` events. */
export interface SpaceAccessCompletedPayload {
  sessionId: string
  peerId: string
  success: boolean
  reason?: string | null
  ts: number
}

/** Payload for `peers.changed` events. */
export interface PeersChangedPayload {
  peers: Array<{
    peerId: string
    deviceName?: string | null
    connected: boolean
  }>
}

/** Payload for `peers.nameUpdated` events. */
export interface PeersNameUpdatedPayload {
  peerId: string
  deviceName: string
}

/** Payload for `peers.connectionChanged` events. */
export interface PeersConnectionChangedPayload {
  peerId: string
  deviceName?: string | null
  connected: boolean
}

// ── Type guard functions ────────────────────────────────────────

export function isSpaceAccessCompletedPayload(
  payload: unknown
): payload is SpaceAccessCompletedPayload {
  if (typeof payload !== 'object' || payload === null) return false
  return 'sessionId' in payload && 'peerId' in payload && 'success' in payload
}

export function isPeersChangedPayload(payload: unknown): payload is PeersChangedPayload {
  if (typeof payload !== 'object' || payload === null) return false
  return 'peers' in payload && Array.isArray((payload as PeersChangedPayload).peers)
}

export function isPeersNameUpdatedPayload(payload: unknown): payload is PeersNameUpdatedPayload {
  if (typeof payload !== 'object' || payload === null) return false
  return 'peerId' in payload && 'deviceName' in payload
}

export function isPeersConnectionChangedPayload(
  payload: unknown
): payload is PeersConnectionChangedPayload {
  if (typeof payload !== 'object' || payload === null) return false
  return 'peerId' in payload && 'connected' in payload
}

// ── useClipboardNewContent ───────────────────────────────────────

/**
 * Subscribe to `clipboard.new_content` events from the daemon WebSocket.
 *
 * The daemon emits this when a new clipboard entry is created locally or synced
 * from a remote device.
 *
 * @param callback  Called with the new clipboard entry each time it arrives.
 *
 * @example
 * useClipboardNewContent((entry) => {
 *   dispatch(prependItem(transformDtoToItemResponse(entry)))
 * })
 */
export function useClipboardNewContent(callback: (entry: ClipboardEntryDto) => void): void {
  const callbackRef = useRef(callback)
  // eslint-disable-next-line react-hooks/refs -- intentional: ref updates stabilize callbacks without re-running effect
  callbackRef.current = callback

  useEffect(() => {
    const handler = (event: DaemonWsEvent<ClipboardNewContentPayload>) => {
      if (event.eventType === 'clipboard.new_content') {
        callbackRef.current(event.payload.entry)
      }
    }

    const unsubscribe = daemonWs.subscribe(['clipboard'], handler)
    return unsubscribe
  }, [])
}

// ── usePairingEvents ─────────────────────────────────────────────

/** Callbacks for pairing lifecycle events. */
export interface UsePairingEventsCallbacks {
  /**
   * Called when the pairing request arrives from a remote device.
   * (kind: 'request')
   */
  onRequest?: (data: { sessionId: string; peerId?: string; deviceName?: string }) => void

  /**
   * Called when the user must verify the pairing with a short code.
   * (kind: 'verification')
   */
  onVerification?: (data: {
    sessionId: string
    peerId?: string
    deviceName?: string
    code?: string
    localFingerprint?: string
    peerFingerprint?: string
  }) => void

  /**
   * Called when the pairing is verifying / processing.
   * (kind: 'verifying')
   */
  onVerifying?: (data: { sessionId: string; peerId?: string; deviceName?: string }) => void

  /**
   * Called when the pairing completes successfully.
   * (kind: 'complete')
   */
  onComplete?: (data: { sessionId: string; peerId?: string; deviceName?: string }) => void

  /**
   * Called when the pairing fails.
   * (kind: 'failed')
   */
  onFailed?: (data: { sessionId: string; error?: string }) => void

  /**
   * Called when a space access is completed after join flow.
   * (topic: 'setup', eventType: 'setup.spaceAccessCompleted')
   */
  onSpaceAccessCompleted?: (data: {
    sessionId: string
    peerId: string
    success: boolean
    reason?: string
  }) => void
}

/**
 * Subscribe to pairing lifecycle events from the daemon WebSocket.
 *
 * Covers the full pairing state machine:
 * - `pairing.updated` (request / verifying)
 * - `pairing.verification_required`
 * - `pairing.complete`
 * - `pairing.failed`
 *
 * Multiple concurrent pairing sessions are supported — callbacks are invoked
 * for all events; callers should filter by `sessionId` if needed.
 *
 * @param callbacks  Object mapping event kinds to handler functions.
 *
 * @example
 * usePairingEvents({
 *   onVerification: ({ sessionId, code, deviceName }) => { ... },
 *   onComplete: ({ sessionId }) => { ... },
 *   onFailed: ({ sessionId, error }) => { ... },
 * })
 */
export function usePairingEvents(callbacks: UsePairingEventsCallbacks): void {
  const callbacksRef = useRef(callbacks)
  // eslint-disable-next-line react-hooks/refs -- intentional: ref updates stabilize callbacks without re-running effect
  callbacksRef.current = callbacks

  useEffect(() => {
    const pairingHandler = (event: DaemonWsEvent) => {
      if (event.topic !== 'pairing') return

      const cbs = callbacksRef.current

      const p = event.payload as {
        sessionId: string
        peerId?: string
        deviceName?: string
        state?: string
        kind?: string
        code?: string
        localFingerprint?: string
        peerFingerprint?: string
        error?: string
        reason?: string
      }

      if (event.eventType === 'pairing.updated') {
        if (p.state === 'request' && cbs.onRequest) {
          logPairingRouting('routed', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            state: p.state,
          })
          cbs.onRequest({ sessionId: p.sessionId, peerId: p.peerId, deviceName: p.deviceName })
        } else if (p.state === 'verifying' && cbs.onVerifying) {
          logPairingRouting('routed', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            state: p.state,
          })
          cbs.onVerifying({ sessionId: p.sessionId, peerId: p.peerId, deviceName: p.deviceName })
        } else {
          logPairingRouting('ignored', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            state: p.state,
            reason: p.state ? 'no_callback_for_state' : 'missing_state_field',
          })
        }
        return
      }

      if (event.eventType === 'pairing.verification_required') {
        if (p.kind === 'verifying' && cbs.onVerifying) {
          logPairingRouting('routed', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            kind: p.kind,
          })
          cbs.onVerifying({ sessionId: p.sessionId, peerId: p.peerId, deviceName: p.deviceName })
          return
        }

        if (p.kind === 'complete' && cbs.onComplete) {
          logPairingRouting('routed', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            kind: p.kind,
          })
          cbs.onComplete({ sessionId: p.sessionId, peerId: p.peerId, deviceName: p.deviceName })
          return
        }

        if (p.kind === 'failed' && cbs.onFailed) {
          logPairingRouting('routed', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            kind: p.kind,
          })
          cbs.onFailed({
            sessionId: p.sessionId,
            error: p.reason ?? p.error,
          })
          return
        }

        if (cbs.onVerification) {
          logPairingRouting('routed', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            kind: p.kind ?? 'verification',
          })
          cbs.onVerification({
            sessionId: p.sessionId,
            peerId: p.peerId,
            deviceName: p.deviceName,
            code: p.code,
            localFingerprint: p.localFingerprint,
            peerFingerprint: p.peerFingerprint,
          })
        } else {
          logPairingRouting('ignored', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            kind: p.kind ?? 'verification',
            reason: 'no_onVerification_callback',
          })
        }
        return
      }

      if (event.eventType === 'pairing.complete') {
        if (cbs.onComplete) {
          logPairingRouting('routed', { eventType: event.eventType, sessionId: p.sessionId })
          cbs.onComplete({ sessionId: p.sessionId, peerId: p.peerId, deviceName: p.deviceName })
        } else {
          logPairingRouting('ignored', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            reason: 'no_onComplete_callback',
          })
        }
        return
      }

      if (event.eventType === 'pairing.failed') {
        if (cbs.onFailed) {
          logPairingRouting('routed', { eventType: event.eventType, sessionId: p.sessionId })
          cbs.onFailed({ sessionId: p.sessionId, error: p.reason })
        } else {
          logPairingRouting('ignored', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            reason: 'no_onFailed_callback',
          })
        }
        return
      }

      logPairingRouting('unsupported', {
        eventType: event.eventType,
        sessionId: p.sessionId,
        reason: 'unrecognised_event_type',
      })
    }

    const setupHandler = (event: DaemonWsEvent) => {
      if (event.topic !== 'setup') return
      if (event.eventType === 'setup.spaceAccessCompleted') {
        if (!isSpaceAccessCompletedPayload(event.payload)) {
          logPairingRouting('unsupported', {
            eventType: event.eventType,
            reason: 'malformed_payload_missing_required_fields',
          })
          return
        }
        const p = event.payload as SpaceAccessCompletedPayload
        if (callbacksRef.current.onSpaceAccessCompleted) {
          logPairingRouting('routed', { eventType: event.eventType, sessionId: p.sessionId })
          callbacksRef.current.onSpaceAccessCompleted({
            sessionId: p.sessionId,
            peerId: p.peerId,
            success: p.success,
            reason: p.reason ?? undefined,
          })
        } else {
          logPairingRouting('ignored', {
            eventType: event.eventType,
            sessionId: p.sessionId,
            reason: 'no_onSpaceAccessCompleted_callback',
          })
        }
      } else {
        logPairingRouting('unsupported', {
          eventType: event.eventType,
          reason: 'unrecognised_setup_event_type',
        })
      }
    }

    const unsubPairing = daemonWs.subscribe(['pairing'], pairingHandler)
    const unsubSetup = daemonWs.subscribe(['setup'], setupHandler)
    return () => {
      unsubPairing()
      unsubSetup()
    }
  }, [])
}

// ── useEncryptionState ───────────────────────────────────────────

/**
 * Subscribe to encryption session state events from the daemon WebSocket.
 *
 * @param onReady   Called when the encryption session becomes ready.
 * @param onFailed  Called when the encryption session fails to initialize (never called —
 *                  daemon does not emit `encryption.session_failed`; failures surface via
 *                  polling fallback instead).
 *
 * @example
 * const { encryptionReady } = useEncryptionSessionState()
 *
 * // or, for more granular control:
 * useEncryptionState({
 *   onReady: () => dispatch(setEncryptionReady(true)),
 *   onFailed: () => dispatch(setEncryptionReady(false)),
 * })
 */
export function useEncryptionState(onReady: () => void, onFailed: () => void): void {
  const onReadyRef = useRef(onReady)
  const onFailedRef = useRef(onFailed)
  // eslint-disable-next-line react-hooks/refs -- intentional: ref updates stabilize callbacks without re-running effect
  onReadyRef.current = onReady
  // eslint-disable-next-line react-hooks/refs -- intentional: ref updates stabilize callbacks without re-running effect
  onFailedRef.current = onFailed

  useEffect(() => {
    const handler = (event: DaemonWsEvent) => {
      if (event.topic !== 'encryption') return

      if (event.eventType === 'encryption.session_ready') {
        onReadyRef.current()
        return
      }

      // Note: encryption.session_failed is never emitted by the daemon.
      // onFailedRef is retained for symmetry but will never fire via WS.
      // Failures surface through the polling fallback in useEncryptionSessionState.
      void onFailedRef
    }

    const unsubscribe = daemonWs.subscribe(['encryption'], handler)
    return unsubscribe
  }, [])
}
