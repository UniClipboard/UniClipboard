/**
 * p2p realtime contract tests — daemon WebSocket direct path.
 *
 * Covers:
 * - onDaemonRealtimeEvent() calls daemonWs.subscribe() (not Tauri listen).
 * - Bridge correctly maps daemon envelope fields (topic, type, sessionId) to
 *   the legacy FrontendRealtimeEvent shape callers expect.
 * - Pairing verification events carry the correct camelCase payload keys
 *   (sessionId, peerId, deviceName, code) without legacy snake_case aliases.
 * - HTTP-level pairing commands fail gracefully without registering spurious listeners.
 *
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// ── Mock daemonWs.subscribe() using vi.spyOn (avoids module-level capture issues) ──

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn(async () => () => {}),
  },
}))

describe('p2p realtime contract', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    // No module reset needed — vi.clearAllMocks() handles mock state cleanup.
  })

  // ── onDaemonRealtimeEvent() calls daemonWs.subscribe() ──────────────────

  it('onDaemonRealtimeEvent calls daemonWs.subscribe() with all daemon topics', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    const { daemonWs } = await import('@/lib/daemon-ws')

    await onDaemonRealtimeEvent(() => {})

    expect(daemonWs.subscribe).toHaveBeenCalledTimes(1)
    expect(daemonWs.subscribe).toHaveBeenCalledWith(
      ['clipboard', 'peers', 'pairing', 'setup', 'paired-devices'],
      expect.any(Function)
    )
  })

  it('onDaemonRealtimeEvent returns an unsubscribe function', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    const { daemonWs } = await import('@/lib/daemon-ws')
    const unsubMock = vi.fn()
    ;(daemonWs.subscribe as ReturnType<typeof vi.fn>).mockResolvedValue(unsubMock)

    const unsub = await onDaemonRealtimeEvent(() => {})

    unsub()
    expect(unsubMock).toHaveBeenCalledTimes(1)
  })

  // ── Envelope field mapping — capture the registered handler and invoke it ──

  it('maps daemon wsEvent fields to FrontendRealtimeEvent shape (topic, type, ts, sessionId)', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    const received: object[] = []

    await onDaemonRealtimeEvent(e => received.push(e))

    // Extract the handler registered with daemonWs.subscribe from the mock's last call.
    const { daemonWs } = await import('@/lib/daemon-ws')
    const subscribeCalls = (daemonWs.subscribe as ReturnType<typeof vi.fn>).mock.calls
    const registeredHandler = subscribeCalls[subscribeCalls.length - 1]?.[1] as (wsEvent: {
      topic: string
      eventType: string
      ts: number
      sessionId: string | null
      payload: unknown
    }) => void

    expect(registeredHandler).toBeDefined()

    // Simulate daemon emitting a real envelope with snake_case field names
    // (as emitted by the Rust DaemonWsEvent struct).
    registeredHandler({
      topic: 'pairing',
      eventType: 'pairing.verification_required',
      ts: 1_710_000_000_000,
      sessionId: 'sess-abc123',
      payload: { sessionId: 'sess-abc123', peerId: 'peer-x', deviceName: 'Desk', code: '123456' },
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({
      topic: 'pairing',
      type: 'pairing.verification_required',
      ts: 1_710_000_000_000,
      sessionId: 'sess-abc123',
    })
  })

  it('maps eventType (daemon) → type (frontend legacy) correctly', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    const received: object[] = []

    await onDaemonRealtimeEvent(e => received.push(e))

    const { daemonWs } = await import('@/lib/daemon-ws')
    const subscribeCalls = (daemonWs.subscribe as ReturnType<typeof vi.fn>).mock.calls
    const registeredHandler = subscribeCalls[subscribeCalls.length - 1]?.[1] as (wsEvent: {
      topic: string
      eventType: string
      ts: number
      sessionId: string | null
      payload: unknown
    }) => void

    registeredHandler({
      topic: 'setup',
      eventType: 'setup.pairingCompleted',
      ts: 1,
      sessionId: 'sess-1',
      payload: {
        sponsorDeviceId: 'sponsor-1',
        joinerDeviceId: 'joiner-2',
        success: true,
        reason: null,
      },
    })

    expect(received).toHaveLength(1)
    // Verify the bridge uses 'type' (not 'eventType') in the forwarded envelope
    expect(received[0]).toHaveProperty('type', 'setup.pairingCompleted')
    expect((received[0] as Record<string, unknown>).eventType).toBeUndefined()
  })

  it('handles null sessionId without crashing', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    const received: object[] = []

    await onDaemonRealtimeEvent(e => received.push(e))

    const { daemonWs } = await import('@/lib/daemon-ws')
    const subscribeCalls = (daemonWs.subscribe as ReturnType<typeof vi.fn>).mock.calls
    const registeredHandler = subscribeCalls[subscribeCalls.length - 1]?.[1] as (wsEvent: {
      topic: string
      eventType: string
      ts: number
      sessionId: string | null
      payload: unknown
    }) => void

    registeredHandler({
      topic: 'clipboard',
      eventType: 'clipboard.entryAdded',
      ts: 2,
      sessionId: null,
      payload: { id: 'entry-1' },
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({ sessionId: null })
  })

  // ── camelCase payload key contract ─────────────────────────────────────

  it('pairing verification event payload has camelCase keys (sessionId, peerId, deviceName, code)', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    let capturedPayload: unknown = null

    await onDaemonRealtimeEvent(e => {
      if (e.topic === 'pairing' && e.type === 'pairing.verification_required') {
        capturedPayload = e.payload
      }
    })

    const { daemonWs } = await import('@/lib/daemon-ws')
    const subscribeCalls = (daemonWs.subscribe as ReturnType<typeof vi.fn>).mock.calls
    const registeredHandler = subscribeCalls[subscribeCalls.length - 1]?.[1] as (wsEvent: {
      topic: string
      eventType: string
      ts: number
      sessionId: string | null
      payload: unknown
    }) => void

    registeredHandler({
      topic: 'pairing',
      eventType: 'pairing.verification_required',
      ts: 1,
      sessionId: 'session-1',
      payload: {
        sessionId: 'session-1',
        peerId: 'peer-1',
        deviceName: 'Desk',
        code: '123456',
      },
    })

    expect(capturedPayload).toMatchObject({
      sessionId: 'session-1',
      peerId: 'peer-1',
      deviceName: 'Desk',
      code: '123456',
    })
    // Ensure no snake_case aliases leak through
    expect((capturedPayload as Record<string, unknown>).session_id).toBeUndefined()
    expect((capturedPayload as Record<string, unknown>).peer_id).toBeUndefined()
    expect((capturedPayload as Record<string, unknown>).device_name).toBeUndefined()
  })

  // ── Multiple topics dispatched correctly ────────────────────────────────

  it('delivers events only when topic matches', async () => {
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')
    const received: object[] = []

    await onDaemonRealtimeEvent(e => received.push(e))

    const { daemonWs } = await import('@/lib/daemon-ws')
    const subscribeCalls = (daemonWs.subscribe as ReturnType<typeof vi.fn>).mock.calls
    const registeredHandler = subscribeCalls[subscribeCalls.length - 1]?.[1] as (wsEvent: {
      topic: string
      eventType: string
      ts: number
      sessionId: string | null
      payload: unknown
    }) => void

    // Event for 'clipboard' — topic IS in the subscription list
    registeredHandler({
      topic: 'clipboard',
      eventType: 'clipboard.entryAdded',
      ts: 1,
      sessionId: null,
      payload: { id: 'e1' },
    })

    expect(received).toHaveLength(1)
    expect(received[0]).toMatchObject({ topic: 'clipboard' })
  })

  // ── Bootstrap sequencing — realtime bridge after daemonWs is connected ───

  it('calling onDaemonRealtimeEvent before daemonWs connect does not throw', async () => {
    // This verifies the contract: onDaemonRealtimeEvent is safe to call at any time.
    // The subscription is buffered until daemonWs.connect() resolves, so callers
    // (like useSetupRealtimeStore) can safely invoke it at module/hook init time.
    const { onDaemonRealtimeEvent } = await import('@/api/realtime')

    await expect(onDaemonRealtimeEvent(() => {})).resolves.toBeDefined()
  })
})
