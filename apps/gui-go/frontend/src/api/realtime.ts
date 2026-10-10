/**
 * Realtime event bridge — frontend WebSocket direct connection.
 *
 * Uses `daemonWs.subscribe()` to listen for daemon WS events.
 *
 * Requires `daemonWs` to be connected first (call `connectDaemonWs()` at startup).
 */

import { daemonWs } from '@/lib/daemon-ws'

/**
 * Event envelope forwarded to callers.
 *
 *   - `topic`  — string like "peers", "pairing", "setup", "clipboard"
 *   - `type`  — snake_case event type like "peers.changed", "pairing.complete"
 *   - `ts`    — Unix timestamp (milliseconds)
 *   - `payload` — raw event payload (varies by topic)
 */
export interface DaemonRealtimeEnvelope<TPayload = unknown> {
  topic: string
  type: string
  ts: number
  sessionId: string | null
  payload: TPayload
}

export type FrontendRealtimeEvent = DaemonRealtimeEnvelope

/**
 * Subscribe to the daemon WebSocket realtime event stream.
 *
 * Requires `daemonWs` to be connected (call `connectDaemonWs()` at startup).
 * If not yet connected, callbacks will fire once the socket opens and the
 * subscribe message is sent to the daemon.
 *
 * @param callback Called for every daemon WS event on any topic.
 * @returns Unsubscribe function — call it to remove the callback.
 */
export async function onDaemonRealtimeEvent(
  callback: (event: FrontendRealtimeEvent) => void
): Promise<() => void> {
  // Convert DaemonWsEvent (eventType) to the legacy envelope (type) so all
  // existing callers continue to work without changes.
  const handler = (wsEvent: {
    topic: string
    eventType: string
    ts: number
    sessionId: string | null
    payload: unknown
  }) => {
    callback({
      topic: wsEvent.topic,
      type: wsEvent.eventType,
      ts: wsEvent.ts,
      sessionId: wsEvent.sessionId,
      payload: wsEvent.payload,
    })
  }

  // Subscribe to all topics that the daemon emits realtime events for.
  const topics = ['clipboard', 'peers', 'pairing', 'setup', 'paired-devices']

  return daemonWs.subscribe(topics, handler)
}
