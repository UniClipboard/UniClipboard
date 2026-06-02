/**
 * DaemonWsClient — singleton WebSocket client for the daemon event stream.
 *
 * Daemon 事件流的 WebSocket 客户端单例，管理连接生命周期、重连、topic 订阅和心跳检测。
 *
 * # Connection Flow / 连接流程
 * 1. `connect(wsUrl)` refreshes the daemon session, then opens a WebSocket to the daemon's
 *    /ws endpoint.
 *    The daemon extracts the session token from the `Authorization: Session <token>` header
 *    (the token comes from `daemonClient.currentSession.token`).
 * 2. `subscribe(topics, callback)` sends a `{ action: "subscribe", topics: [...], nonce }`
 *    message over the socket. The daemon replies with snapshot events for each topic.
 * 3. Subsequent events matching subscribed topics are dispatched to registered callbacks.
 * 4. On disconnect, the client retries with exponential backoff (1s → 30s, max 10 attempts).
 * 5. On reconnect, previously subscribed topics are automatically re-subscribed.
 *
 * # Heartbeat / 心跳
 * - The daemon server sends protocol-level WebSocket `Ping` frames every 30s.
 * - Browsers automatically reply with `Pong`, but the browser WebSocket API
 *   does not expose ping/pong frames to JavaScript.
 * - Because of that limitation, the frontend must not treat "no app-level
 *   message for N ms" as a dead connection; an idle but healthy socket would
 *   otherwise be misclassified and force an unnecessary reconnect.
 * - Reconnect is therefore driven by actual `onclose` / `onerror` signals.
 */

import { daemonClient } from '@/api/daemon/client'
import { createLogger } from '@/lib/logger'

const log = createLogger('daemon-ws')

// ── Public types ────────────────────────────────────────────────

/**
 * Raw event envelope received from the daemon WebSocket.
 * Rust `DaemonWsEvent` serializes with `#[serde(rename_all = "camelCase")]`
 * and `#[serde(rename = "type")]` for `event_type`, so the wire format is:
 * `{ topic, type, sessionId, ts, payload }`.
 *
 * 从 daemon WebSocket 接收的事件信封。
 */
export interface DaemonWsEvent<T = unknown> {
  topic: string
  eventType: string
  ts: number
  sessionId: string | null
  payload: T
}

/** Callback registered per-topic. */
export type WsEventCallback<T = unknown> = (event: DaemonWsEvent<T>) => void

// ── Constants ───────────────────────────────────────────────────

const RECONNECT_BASE_DELAY_MS = 1_000
const RECONNECT_MAX_DELAY_MS = 30_000
const MAX_RECONNECT_ATTEMPTS = 10

// ── Internal helpers ────────────────────────────────────────────

/** Generate a short random nonce for subscribe requests. */
function makeNonce(): string {
  return Math.random().toString(36).slice(2, 10)
}

function topicMatches(subscription: string, eventTopic: string): boolean {
  return subscription === eventTopic
}

// ── DaemonWsClient ──────────────────────────────────────────────

/** DaemonWsClient class — exported for testability (use `daemonWs` singleton in production). */
export class DaemonWsClient {
  /**
   * WebSocket constructor used to open connections.
   * Exposed for testability — defaults to the global WebSocket.
   */
  protected readonly _wsFactory: (url: string) => WebSocket

  private _ws: WebSocket | null = null
  private _wsUrl: string | null = null

  /** Registered callbacks keyed by topic. */
  private _callbacks = new Map<string, Set<WsEventCallback>>()

  /** Topics that are currently active (for re-subscribe on reconnect). */
  private _activeTopics = new Set<string>()

  /** Pending connect() resolver / rejecter. */
  private _connectResolve: (() => void) | null = null
  private _connectReject: ((err: Error) => void) | null = null

  /** Reconnect state. */
  private _reconnectAttempt = 0
  private _reconnectTimer: ReturnType<typeof setTimeout> | null = null
  /** Guard against overlapping reconnect attempts. */
  private _isReconnecting = false

  constructor(wsFactory?: (url: string) => WebSocket) {
    this._wsFactory = wsFactory ?? (url => new WebSocket(url))
  }

  // ── Public API ────────────────────────────────────────────────

  /**
   * Open a WebSocket connection to the daemon.
   *
   * Refreshes the daemon session before opening the socket, then uses
   * `daemonClient.currentSession`.
   * Resolves when the socket is open; rejects on connection failure.
   * If already connected, resolves immediately.
   *
   * @param wsUrl WebSocket URL, e.g. `ws://127.0.0.1:42715/ws`
   */
  connect(wsUrl: string): Promise<void> {
    return new Promise((resolve, reject) => {
      if (this._ws && this._ws.readyState === WebSocket.OPEN) {
        resolve()
        return
      }

      this._wsUrl = wsUrl

      this._connectResolve = resolve
      this._connectReject = reject

      this._openSocket().catch(err => {
        log.error({ err }, 'failed to open daemon WebSocket')
        const rejectConnect = this._connectReject
        this._connectResolve = null
        this._connectReject = null
        if (rejectConnect) {
          rejectConnect(err instanceof Error ? err : new Error('failed to open daemon WebSocket'))
        }
      })
    })
  }

  /**
   * Close the WebSocket and cancel any scheduled reconnect.
   */
  disconnect(): void {
    this._cancelReconnect()
    this._reconnectAttempt = 0
    this._isReconnecting = false
    if (this._ws) {
      this._ws.onopen = null
      this._ws.onmessage = null
      this._ws.onerror = null
      this._ws.onclose = null
      this._ws.close()
      this._ws = null
    }
    this._wsUrl = null
  }

  /**
   * Subscribe to one or more daemon topics.
   *
   * Sends a subscribe message over the socket. The daemon immediately responds
   * with a snapshot event for each topic, followed by incremental events as they occur.
   *
   * @param topics  Topic names (e.g. "clipboard", "encryption", "peers").
   * @param callback Called for every event matching any subscribed topic.
   * @returns Unsubscribe function. Call it to remove the callback without closing the socket.
   */
  subscribe<T = unknown>(topics: string[], callback: WsEventCallback<T>): () => void {
    const cb = callback as WsEventCallback

    for (const topic of topics) {
      this._activeTopics.add(topic)
      if (!this._callbacks.has(topic)) {
        this._callbacks.set(topic, new Set())
      }
      this._callbacks.get(topic)!.add(cb)
    }

    // If the socket is open, send the subscribe request now.
    if (this._ws && this._ws.readyState === WebSocket.OPEN) {
      this._sendSubscribe(topics)
    }

    // Return an unsubscribe function.
    return () => {
      this._unsubscribe(topics, cb)
    }
  }

  /**
   * Reset all internal state — used for test cleanup.
   *
   * 清除所有内部状态 — 供测试清理使用。
   */
  reset(): void {
    // Clear _wsUrl first so that any stray close events from old sockets
    // won't trigger reconnect (the guard in _scheduleReconnect checks !this._wsUrl).
    this._wsUrl = null
    this._cancelReconnect()
    if (this._ws) {
      this._ws.onopen = null
      this._ws.onmessage = null
      this._ws.onerror = null
      this._ws.onclose = null
      this._ws.close()
      this._ws = null
    }
    this._callbacks.clear()
    this._activeTopics.clear()
    this._connectResolve = null
    this._connectReject = null
    this._reconnectAttempt = 0
    this._isReconnecting = false
  }

  // ── Private helpers ────────────────────────────────────────────

  private async _openSocket(): Promise<void> {
    await daemonClient.refreshSession()

    if (!this._wsUrl) {
      return
    }

    const token = daemonClient.currentSession?.token

    // Close any previous socket before opening a new one.
    if (this._ws) {
      this._ws.onclose = null
      this._ws.close()
    }

    // Build the Authorization header value by putting it in the URL query param
    // (standard browsers don't allow custom headers on WebSocket connections).
    const url = this._wsUrl!
    const authUrl = token ? `${url}?auth=${encodeURIComponent(`Session ${token}`)}` : url

    const ws = this._wsFactory(authUrl)
    this._ws = ws

    ws.onopen = () => {
      this._reconnectAttempt = 0
      this._isReconnecting = false
      const r = this._connectResolve
      this._connectResolve = null
      this._connectReject = null
      if (r) r()
      // Re-subscribe any topics that were registered before the socket opened.
      if (this._activeTopics.size > 0) {
        this._sendSubscribe([...this._activeTopics])
      }
    }

    ws.onmessage = event => {
      try {
        this._handleMessage(event.data)
      } catch (err) {
        log.error({ err }, 'failed to handle incoming message')
      }
    }

    ws.onerror = event => {
      log.error({ event }, 'WebSocket error')
      const reject = this._connectReject
      this._connectResolve = null
      this._connectReject = null
      if (reject) {
        reject(new Error('WebSocket error'))
      }
    }

    ws.onclose = () => {
      const reject = this._connectReject
      this._connectResolve = null
      this._connectReject = null
      if (reject) {
        reject(new Error('WebSocket closed before open'))
      }
      // Schedule a reconnect unless this was a clean disconnect().
      if (this._wsUrl) {
        this._scheduleReconnect()
      }
    }
  }

  private _handleMessage(data: string): void {
    // Parse the raw event from the daemon.
    // The daemon serializes with camelCase + rename: { topic, type, sessionId, ts, payload }
    let raw: Record<string, unknown>
    try {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      raw = JSON.parse(data) as Record<string, any>
    } catch {
      log.error({ data }, 'failed to parse incoming message')
      return
    }

    const event: DaemonWsEvent = {
      topic: raw.topic as string,
      eventType: (raw.type ?? raw.event_type) as string,
      ts: raw.ts as number,
      sessionId: (raw.sessionId ?? raw.session_id ?? null) as string | null,
      payload: raw.payload,
    }
    log.debug({ topic: event.topic, eventType: event.eventType }, 'received WS event')

    for (const [subscriptionTopic, callbacks] of this._callbacks.entries()) {
      if (!topicMatches(subscriptionTopic, event.topic)) {
        continue
      }
      for (const cb of callbacks) {
        try {
          cb(event)
        } catch (err) {
          log.error({ err, topic: subscriptionTopic }, 'event callback threw')
        }
      }
    }
  }

  private _sendSubscribe(topics: string[]): void {
    if (!this._ws || this._ws.readyState !== WebSocket.OPEN) return
    const msg = {
      action: 'subscribe',
      topics,
      nonce: makeNonce(),
    }
    try {
      this._ws.send(JSON.stringify(msg))
    } catch (err) {
      log.error({ err }, 'failed to send subscribe message')
    }
  }

  private _unsubscribe(topics: string[], callback: WsEventCallback): void {
    for (const topic of topics) {
      const cbs = this._callbacks.get(topic)
      if (!cbs) continue
      cbs.delete(callback)
      if (cbs.size === 0) {
        this._callbacks.delete(topic)
        this._activeTopics.delete(topic)
      }
    }
  }

  private _scheduleReconnect(): void {
    if (this._isReconnecting) return
    // Don't reconnect if disconnect() has cleared _wsUrl since this close was intentional.
    if (!this._wsUrl) return

    if (this._reconnectAttempt >= MAX_RECONNECT_ATTEMPTS) {
      log.error({ maxAttempts: MAX_RECONNECT_ATTEMPTS }, 'gave up reconnecting')
      this._isReconnecting = false
      return
    }

    this._isReconnecting = true
    this._reconnectAttempt++

    // Exponential backoff with jitter: min(30s, 1s * 2^attempt) ± 10%.
    const baseDelay = Math.min(
      RECONNECT_MAX_DELAY_MS,
      RECONNECT_BASE_DELAY_MS * 2 ** (this._reconnectAttempt - 1)
    )
    const jitter = baseDelay * 0.1 * (Math.random() * 2 - 1)
    const delayMs = Math.round(baseDelay + jitter)

    log.info(
      { attempt: this._reconnectAttempt, maxAttempts: MAX_RECONNECT_ATTEMPTS, delayMs },
      'scheduling reconnect'
    )

    this._reconnectTimer = setTimeout(() => {
      this._isReconnecting = false
      if (this._wsUrl) {
        this._openSocket().catch(err => {
          log.error({ err }, 'failed to refresh session before reconnect')
          this._scheduleReconnect()
        })
      }
    }, delayMs)
  }

  private _cancelReconnect(): void {
    if (this._reconnectTimer !== null) {
      clearTimeout(this._reconnectTimer)
      this._reconnectTimer = null
    }
  }

  // ── Heartbeat helpers ─────────────────────────────────────────
}

/** Singleton DaemonWsClient instance (uses global WebSocket). */
export const daemonWs = new DaemonWsClient()
