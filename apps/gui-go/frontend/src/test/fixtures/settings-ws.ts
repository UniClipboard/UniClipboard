// A stand-in for `@/lib/daemon-ws` in tests of `@/lib/settings-sync` consumers:
//   vi.mock('@/lib/daemon-ws', () => import('@/test/fixtures/settings-ws'))
// Like the daemon, every subscribe is answered with `settings.snapshot`, which gates the first read.
import type { DaemonWsEvent, WsEventCallback } from '@/lib/daemon-ws'

const callbacks = new Set<WsEventCallback>()
let connected = true

function event(eventType: string, payload: unknown = {}): DaemonWsEvent {
  return { topic: eventType.split('.')[0]!, eventType, ts: Date.now(), sessionId: null, payload }
}

export const daemonWs = {
  subscribe(_topics: string[], callback: WsEventCallback): () => void {
    callbacks.add(callback)
    queueMicrotask(() => {
      if (connected && callbacks.has(callback)) callback(snapshot())
    })
    return () => {
      callbacks.delete(callback)
    }
  },
  onReconnect(): () => void {
    return () => {}
  },
}

function snapshot(): DaemonWsEvent {
  return event('settings.snapshot', { revisions: { desktop: 0, engine: 0 } })
}

/** Hold snapshots until `connectDaemonWsForTest`, like a socket that is not open yet. */
export function disconnectDaemonWsForTest(): void {
  connected = false
}

/** Open the socket: every subscriber gets its snapshot. */
export function connectDaemonWsForTest(): void {
  connected = true
  for (const callback of callbacks) callback(snapshot())
}

/** Deliver a daemon event, e.g. `settings.changed`, to every subscriber. */
export function emitDaemonWsEvent(eventType: string, payload: unknown = {}): void {
  for (const callback of callbacks) callback(event(eventType, payload))
}
