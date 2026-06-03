/**
 * Integration tests for DaemonWsClient WebSocket event delivery and reconnection.
 *
 * NOTE: vi.useFakeTimers() is called INSIDE reconnect tests only, not globally.
 * Global fake timers replace the global EventTarget, which breaks MockWebSocket
 * when it extends EventTarget. Since reconnect uses setTimeout internally, we
 * activate fake timers per reconnect test and restore real timers afterward.
 *
 * @vitest-environment jsdom
 */

import { afterEach, describe, expect, it, vi } from 'vitest'

// ── Mock WebSocket ─────────────────────────────────────────────
class MockWebSocket {
  static _nextId = 0

  readonly id: number
  readyState: number = 0 // CONNECTING
  url: string = ''
  sentMessages: string[] = []

  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null

  constructor(url: string) {
    this.id = MockWebSocket._nextId++
    this.url = url
  }

  send(data: string): void {
    this.sentMessages.push(data)
  }

  close(): void {
    this.readyState = 3 // CLOSED
    if (this.onclose) this.onclose(new CloseEvent('close', { wasClean: false }))
  }
}

// ── Mock daemonClient ─────────────────────────────────────────
const mockDaemonClient = vi.hoisted(() => {
  let refreshCount = 0
  const client = {
    currentSession: {
      token: 'test-session-token',
      expiresAt: Date.now() + 300_000,
      encryptionReady: false,
    },
    async refreshSession() {
      refreshCount++
      client.currentSession = {
        token: `test-session-token-${refreshCount}`,
        expiresAt: Date.now() + 300_000,
        encryptionReady: false,
      }
      return client.currentSession
    },
    reset() {
      refreshCount = 0
      client.currentSession = {
        token: 'test-session-token',
        expiresAt: Date.now() + 300_000,
        encryptionReady: false,
      }
    },
    get refreshCount() {
      return refreshCount
    },
  }
  return client
})

vi.mock('@/api/daemon/client', () => ({
  daemonClient: mockDaemonClient,
}))

const { DaemonWsClient } = await import('@/lib/daemon-ws')
type DaemonWsEvent = import('@/lib/daemon-ws').DaemonWsEvent
type ClientInstance = InstanceType<typeof DaemonWsClient>

// ── Helpers ─────────────────────────────────────────────────

const OPEN = 1
const CLOSED = 3

/** Fire the socket's onopen handler. Call after client.connect() so client._ws is set. */
function openSocket(ws: MockWebSocket): void {
  ws.readyState = OPEN
  if (ws.onopen) ws.onopen(new Event('open'))
}

/** Simulate an incoming daemon event. */
function receiveMessage(ws: MockWebSocket, payload: object): void {
  if (!ws.onmessage) return
  ws.onmessage(new MessageEvent('message', { data: JSON.stringify(payload) }))
}

/** Simulate unexpected socket close (triggers onclose handler). */
function closeSocket(ws: MockWebSocket): void {
  ws.readyState = CLOSED
  if (ws.onclose) ws.onclose(new CloseEvent('close', { wasClean: false }))
}

function freshClient(): { client: ClientInstance } {
  MockWebSocket._nextId = 0
  mockDaemonClient.reset()
  const client = new DaemonWsClient((url: string) => new MockWebSocket(url) as unknown as WebSocket)
  return { client }
}

function currentWs(client: ClientInstance): MockWebSocket {
  return client['_ws'] as unknown as MockWebSocket
}

async function waitForSocket(client: ClientInstance): Promise<MockWebSocket> {
  await Promise.resolve()
  return currentWs(client)
}

// ── connect() ─────────────────────────────────────────────────

describe('connect()', () => {
  it('resolves when the socket opens successfully', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p
    expect(ws.readyState).toBe(OPEN)
  })

  it('refreshes the session before passing the auth URL to the WebSocket factory', async () => {
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    expect(mockDaemonClient.refreshCount).toBe(1)
    expect(ws.url).toBe('ws://127.0.0.1:42715/ws?auth=Session%20test-session-token-1')
  })

  it('resolves immediately if already connected', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p
    // Second connect while socket is open resolves immediately (no factory call).
    const p2 = client.connect('ws://127.0.0.1:42715/ws')
    await p2
    expect(ws.sentMessages).toHaveLength(0)
  })

  it('rejects if socket closes before opening', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    closeSocket(ws)
    await expect(p).rejects.toThrow('WebSocket closed before open')
  })

  it('rejects on socket error', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    ws.readyState = CLOSED
    if (ws.onerror) ws.onerror(new Event('error'))
    await expect(p).rejects.toThrow('WebSocket error')
  })
})

// ── disconnect() ─────────────────────────────────────────────

describe('disconnect()', () => {
  it('closes the socket and clears reconnect state', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const closeSpy = vi.spyOn(ws, 'close')
    client.disconnect()

    expect(closeSpy).toHaveBeenCalled()
    expect(client['_wsUrl']).toBeNull()
    expect(client['_reconnectAttempt']).toBe(0)
    expect(client['_reconnectTimer']).toBeNull()
  })
})

// ── subscribe() ───────────────────────────────────────────────

describe('subscribe()', () => {
  it('sends a subscribe message with the correct shape', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    client.subscribe(['clipboard'], () => {})

    const msg = JSON.parse(ws.sentMessages[0])
    expect(msg).toMatchObject({
      action: 'subscribe',
      topics: ['clipboard'],
      nonce: expect.any(String),
    })
  })

  it('returns an unsubscribe function', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const cb = vi.fn()
    const unsub = client.subscribe(['clipboard'], cb)

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: { id: 'e1' },
    })
    expect(cb).toHaveBeenCalledTimes(1)

    unsub()

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 2,
      session_id: null,
      payload: { id: 'e2' },
    })
    expect(cb).toHaveBeenCalledTimes(1) // still 1
  })

  it('includes multiple topics in one subscribe message', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    client.subscribe(['clipboard', 'encryption'], () => {})

    const msg = JSON.parse(ws.sentMessages[0])
    expect(msg.topics).toEqual(['clipboard', 'encryption'])
  })

  it('dispatches incoming events to the registered callback', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1710000000000,
      session_id: 'sess-1',
      payload: { id: 'e1' },
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({
      topic: 'clipboard',
      eventType: 'entry_added',
      ts: 1710000000000,
      sessionId: 'sess-1',
      payload: { id: 'e1' },
    })
  })

  it('dispatches pairing events to pairing topic subscribers', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['pairing'], (e: DaemonWsEvent) => received.push(e))

    receiveMessage(ws, {
      topic: 'pairing',
      event_type: 'pairing.updated',
      ts: 1710000000001,
      session_id: 'sess-pairing-1',
      payload: { sessionId: 'sess-pairing-1', status: 'request' },
    })

    receiveMessage(ws, {
      topic: 'pairing',
      event_type: 'pairing.verification_required',
      ts: 1710000000002,
      session_id: 'sess-pairing-1',
      payload: { sessionId: 'sess-pairing-1', code: '123456' },
    })

    expect(received).toHaveLength(2)
    expect(received[0]).toMatchObject({
      topic: 'pairing',
      eventType: 'pairing.updated',
      sessionId: 'sess-pairing-1',
    })
    expect(received[1]).toMatchObject({
      topic: 'pairing',
      eventType: 'pairing.verification_required',
      sessionId: 'sess-pairing-1',
    })
  })

  it('dispatches to multiple callbacks registered for the same topic', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const cb1 = vi.fn()
    const cb2 = vi.fn()

    client.subscribe(['clipboard'], cb1)
    client.subscribe(['clipboard'], cb2)

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: {},
    })

    expect(cb1).toHaveBeenCalledTimes(1)
    expect(cb2).toHaveBeenCalledTimes(1)
  })

  it('handles null session_id without crashing', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: {},
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({ sessionId: null })
  })
})

// ── reconnect ───────────────────────────────────────────────

describe('reconnect', () => {
  // Restore real timers after each test to prevent cross-test contamination.
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('schedules reconnect with exponential backoff when socket closes unexpectedly', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    closeSocket(ws)
    await vi.advanceTimersByTimeAsync(1000)
    expect(client['_reconnectAttempt']).toBe(1)
    expect(mockDaemonClient.refreshCount).toBe(2)
  })

  it('does NOT reconnect after disconnect()', async () => {
    vi.useFakeTimers()
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    client.disconnect()
    vi.advanceTimersByTime(2000)
    expect(client['_wsUrl']).toBeNull()
    expect(client['_reconnectAttempt']).toBe(0)
  })

  it('gives up after MAX_RECONNECT_ATTEMPTS (10)', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    closeSocket(ws)

    for (let i = 1; i <= 10; i++) {
      expect(client['_reconnectAttempt']).toBe(i)
      await vi.advanceTimersByTimeAsync(Math.min(30000, 1000 * 2 ** (i - 1)))
      const newWs = await waitForSocket(client)
      closeSocket(newWs)
    }

    await vi.advanceTimersByTimeAsync(10000)
    expect(client['_reconnectAttempt']).toBe(10)
    expect(client['_isReconnecting']).toBe(false)
  })

  it('reconnect refreshes the session and auto-resubscribes topics on the new socket', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    client.subscribe(['clipboard', 'encryption'], () => {})
    ws.sentMessages = []

    closeSocket(ws)
    await vi.advanceTimersByTimeAsync(1000)
    const newWs = await waitForSocket(client)
    openSocket(newWs)

    expect(mockDaemonClient.refreshCount).toBe(2)
    expect(newWs.url).toBe('ws://127.0.0.1:42715/ws?auth=Session%20test-session-token-2')
    const msg = JSON.parse(newWs.sentMessages[0])
    expect(msg.topics).toEqual(['clipboard', 'encryption'])
    expect(client['_reconnectAttempt']).toBe(0)
  })

  it('receives events on the new socket after reconnect', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    closeSocket(ws)
    await vi.advanceTimersByTimeAsync(1000)
    const newWs = await waitForSocket(client)
    openSocket(newWs)

    receiveMessage(newWs, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: { id: 'after-reconnect' },
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({ payload: { id: 'after-reconnect' } })
  })

  it('does not reconnect just because the socket is idle with no app-level messages', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    vi.advanceTimersByTime(5 * 60_000)

    expect(client['_reconnectAttempt']).toBe(0)
    expect(client['_ws']).toBe(ws as unknown as WebSocket)
    expect(ws.readyState).toBe(OPEN)
  })
})

// ── onReconnect ─────────────────────────────────────────────

describe('onReconnect', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('does NOT fire on the initial connect', async () => {
    const { client } = freshClient()
    const cb = vi.fn()
    client.onReconnect(cb)

    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    expect(cb).not.toHaveBeenCalled()
  })

  it('fires after a genuine reconnect', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    const cb = vi.fn()
    client.onReconnect(cb)

    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()
    expect(cb).not.toHaveBeenCalled()

    closeSocket(ws)
    await vi.advanceTimersByTimeAsync(1000)
    const newWs = await waitForSocket(client)
    openSocket(newWs)

    expect(cb).toHaveBeenCalledTimes(1)
  })

  it('returns an unregister function that stops further callbacks', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const { client } = freshClient()
    const cb = vi.fn()
    const unregister = client.onReconnect(cb)

    void client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    vi.runAllTicks()

    unregister()

    closeSocket(ws)
    await vi.advanceTimersByTimeAsync(1000)
    const newWs = await waitForSocket(client)
    openSocket(newWs)

    expect(cb).not.toHaveBeenCalled()
  })

  it('does not fire again on a fresh connect after disconnect()', async () => {
    const { client } = freshClient()
    const cb = vi.fn()
    client.onReconnect(cb)

    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    client.disconnect()

    const p2 = client.connect('ws://127.0.0.1:42715/ws')
    const ws2 = await waitForSocket(client)
    openSocket(ws2)
    await p2

    // disconnect() reset the "connected once" flag, so this counts as an
    // initial connect, not a reconnect.
    expect(cb).not.toHaveBeenCalled()
  })
})

// ── Topic filtering ─────────────────────────────────────────

describe('Topic filtering', () => {
  it('delivers clipboard events only to clipboard subscribers', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const clipboardEvents: object[] = []
    const encryptionEvents: object[] = []

    client.subscribe(['clipboard'], (e: DaemonWsEvent) => clipboardEvents.push(e))
    client.subscribe(['encryption'], (e: DaemonWsEvent) => encryptionEvents.push(e))

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: {},
    })

    expect(clipboardEvents).toHaveLength(1)
    expect(encryptionEvents).toHaveLength(0)
  })

  it('delivers encryption lock/unlock events', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['encryption'], (e: DaemonWsEvent) => received.push(e))

    receiveMessage(ws, {
      topic: 'encryption',
      event_type: 'session_ready',
      ts: 1,
      session_id: 'sess-1',
      payload: { ready: true },
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({ eventType: 'session_ready', payload: { ready: true } })
  })

  it('does not deliver events for topics with no subscribers', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    receiveMessage(ws, {
      topic: 'peers',
      event_type: 'peer_connected',
      ts: 1,
      session_id: null,
      payload: {},
    })

    expect(received).toHaveLength(0)
  })
})

// ── Error resilience ────────────────────────────────────────

describe('Error resilience', () => {
  it('does not crash when JSON parse fails', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    expect(() => {
      client['_handleMessage']('not-valid-json{{{')
    }).not.toThrow()
  })

  it('does not crash when callback throws', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const goodCb = vi.fn()
    const throwingCb = vi.fn(() => {
      throw new Error('boom')
    })

    client.subscribe(['clipboard'], throwingCb)
    client.subscribe(['clipboard'], goodCb)

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: {},
    })

    expect(throwingCb).toHaveBeenCalledTimes(1)
    expect(goodCb).toHaveBeenCalledTimes(1)
  })
})

// ── Rapid events ───────────────────────────────────────────

describe('Rapid events', () => {
  it('delivers all events when many arrive in rapid succession', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    for (let i = 0; i < 20; i++) {
      receiveMessage(ws, {
        topic: 'clipboard',
        event_type: 'entry_added',
        ts: 1 + i,
        session_id: null,
        payload: { id: i },
      })
    }

    expect(received).toHaveLength(20)
  })

  it('unsubscribe stops delivery of subsequent events', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    const unsub = client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    receiveMessage(ws, {
      topic: 'clipboard',
      event_type: 'entry_added',
      ts: 1,
      session_id: null,
      payload: { id: 'e1' },
    })
    expect(received).toHaveLength(1)

    unsub()

    for (let i = 0; i < 10; i++) {
      receiveMessage(ws, {
        topic: 'clipboard',
        event_type: 'entry_added',
        ts: 2 + i,
        session_id: null,
        payload: { id: `e-after-${i}` },
      })
    }

    expect(received).toHaveLength(1)
  })
})

// ── Event latency ─────────────────────────────────────────

describe('Event latency', () => {
  it('delivers events synchronously with no artificial delay', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    for (let i = 0; i < 10; i++) {
      receiveMessage(ws, {
        topic: 'clipboard',
        event_type: 'entry_added',
        ts: 1 + i,
        session_id: null,
        payload: { id: `e${i}` },
      })
    }

    expect(received).toHaveLength(10)
  })

  it('events delivered within 100ms when using 80ms spacing', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p

    const received: object[] = []
    client.subscribe(['clipboard'], (e: DaemonWsEvent) => received.push(e))

    for (let i = 0; i < 5; i++) {
      receiveMessage(ws, {
        topic: 'clipboard',
        event_type: 'entry_added',
        ts: 1 + i,
        session_id: null,
        payload: { id: `e${i}` },
      })
      // Use fake timers just for the advance, then switch back.
      vi.useFakeTimers()
      vi.advanceTimersByTime(80)
      vi.useRealTimers()
    }

    expect(received).toHaveLength(5)
  })
})

// ── reset() ───────────────────────────────────────────────

describe('reset()', () => {
  it('clears all state and closes the socket', async () => {
    const { client } = freshClient()
    const p = client.connect('ws://127.0.0.1:42715/ws')
    const ws = await waitForSocket(client)
    openSocket(ws)
    await p
    client.subscribe(['clipboard'], () => {})

    client.reset()

    expect(client['_ws']).toBeNull()
    expect(client['_wsUrl']).toBeNull()
    expect(client['_callbacks'].size).toBe(0)
    expect(client['_activeTopics'].size).toBe(0)
    expect(client['_reconnectAttempt']).toBe(0)
  })
})

// ── Singleton ─────────────────────────────────────────────

describe('daemonWs singleton', () => {
  it('daemonWs is an instance of DaemonWsClient', async () => {
    const { daemonWs: dw } = await import('@/lib/daemon-ws')
    expect(dw).toBeInstanceOf(DaemonWsClient)
  })
})
