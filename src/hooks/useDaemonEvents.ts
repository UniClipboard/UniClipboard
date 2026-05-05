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

/** Payload for `peers.changed` events. */
export interface PeersChangedPayload {
  peers: Array<{
    peerId: string
    deviceName?: string | null
    connected: boolean
    /** Phase 96 INDIC-01: 连接通道 4 态 wire 字符串。 */
    channel?: 'direct' | 'relay' | 'offline' | 'unknown'
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

// Slice 4 P5a-3: usePairingEvents + UsePairingEventsCallbacks +
// SpaceAccessCompletedPayload were removed alongside
// PairingNotificationProvider/PairingDialog. The new setup-v2 flow
// consumes its own events through `src/store/setupRealtimeStore.ts`,
// so nothing in app code subscribes to the legacy `pairing` topic
// anymore. The previous implementation is preserved in git history.

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
