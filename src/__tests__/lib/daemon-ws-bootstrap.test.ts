/**
 * daemon-ws-bootstrap ordering and idempotency tests.
 *
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { resetDaemonConnectionInfoPollingForTests } from '@/lib/daemon-connection-info'

const TEST_PAYLOAD = {
  baseUrl: 'http://127.0.0.1:42715',
  wsUrl: 'ws://127.0.0.1:42715/ws',
}

let _sessionToken: string | null = null
let _fetchQueue: Response[] = []
let _daemonWsConnectCalls: string[] = []

// Now mocks the typed `commands.getDaemonConnectionInfo` proxy from
// `@/lib/ipc` (used to be `invokeWithTrace('get_daemon_connection_info')`
// from `@/lib/tauri-command`). The behaviour each test wants — sequential
// resolved values to simulate the polling — is unchanged; only the seam
// the test reaches into shifted to the typed proxy.
const mockGetDaemonConnectionInfo = vi.fn()

function resetState(): void {
  _sessionToken = null
  _fetchQueue = []
  _daemonWsConnectCalls = []
  mockGetDaemonConnectionInfo.mockReset()
}

function authResponse(sessionToken = 'jwt-from-refresh'): Response {
  return new Response(JSON.stringify({ sessionToken, expiresInSecs: 300, refreshAtSecs: 240 }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function errResponse(status: number): Response {
  return new Response(JSON.stringify({ error: `HTTP ${status}` }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

let refreshSessionCallCount = 0

vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonConnectionInfo: (...args: unknown[]) => mockGetDaemonConnectionInfo(...args),
    // The connection-info poll now also probes for a recorded bootstrap failure
    // each round; these tests exercise the success/auth paths, so a constant
    // "no failure" keeps the poll on its happy path.
    getDaemonBootstrapFailure: async () => null,
  },
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    get initialized() {
      return true
    },
    get wsUrl() {
      return TEST_PAYLOAD.wsUrl
    },
    get currentSession() {
      return _sessionToken
        ? { token: _sessionToken, expiresAt: Date.now() + 300_000, encryptionReady: false }
        : null
    },
    initialize(_config: unknown) {},
    async refreshSession() {
      refreshSessionCallCount++
      const response = _fetchQueue.shift() ?? authResponse()
      if (!response.ok) throw new Error(`refreshSession failed ${response.status}`)
      const data = (await response.json()) as { sessionToken: string; expiresInSecs: number }
      _sessionToken = data.sessionToken
      return {
        token: _sessionToken,
        expiresAt: Date.now() + data.expiresInSecs * 1000,
        encryptionReady: false,
      }
    },
    destroy() {
      _sessionToken = null
      _fetchQueue = []
    },
  },
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    connect: vi.fn(async (url: string) => {
      _daemonWsConnectCalls.push(url)
    }),
    subscribe: vi.fn(async () => () => {}),
  },
}))

const { connectDaemonWs, resetConnectDaemonWsForTests } = await import('@/lib/daemon-ws-bootstrap')

function getDaemonWsConnectCount(): number {
  return _daemonWsConnectCalls.length
}

describe('connectDaemonWs()', () => {
  beforeEach(() => {
    resetConnectDaemonWsForTests()
    resetDaemonConnectionInfoPollingForTests()
    resetState()
    refreshSessionCallCount = 0
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('polls command until connection info is ready before refreshing session and connecting ws', async () => {
    _fetchQueue.push(authResponse('session-jwt-123'))
    mockGetDaemonConnectionInfo.mockResolvedValueOnce(null).mockResolvedValueOnce(TEST_PAYLOAD)

    const promise = connectDaemonWs()
    expect(getDaemonWsConnectCount()).toBe(0)

    await vi.advanceTimersByTimeAsync(500)
    await promise

    expect(mockGetDaemonConnectionInfo).toHaveBeenNthCalledWith(1)
    expect(mockGetDaemonConnectionInfo).toHaveBeenNthCalledWith(2)
    expect(refreshSessionCallCount).toBe(1)
    expect(getDaemonWsConnectCount()).toBe(1)
    expect(_daemonWsConnectCalls[0]).toBe(TEST_PAYLOAD.wsUrl)
  })

  it('shares one polling sequence across concurrent callers', async () => {
    _fetchQueue.push(authResponse())
    mockGetDaemonConnectionInfo.mockResolvedValueOnce(null).mockResolvedValueOnce(TEST_PAYLOAD)

    const first = connectDaemonWs()
    const second = connectDaemonWs()
    const third = connectDaemonWs()

    await vi.advanceTimersByTimeAsync(500)
    await Promise.all([first, second, third])

    expect(mockGetDaemonConnectionInfo).toHaveBeenCalledTimes(2)
    expect(refreshSessionCallCount).toBe(1)
    expect(getDaemonWsConnectCount()).toBe(1)
  })

  it('rejects malformed command payloads before initializing clients', async () => {
    mockGetDaemonConnectionInfo.mockResolvedValue({ baseUrl: 'http://127.0.0.1:9000' })

    await expect(connectDaemonWs()).rejects.toThrow('Malformed daemon connection payload')
    expect(getDaemonWsConnectCount()).toBe(0)
  })

  it('surfaces auth refresh failure without leaking bearer token value', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockReturnValue(undefined)
    _fetchQueue.push(errResponse(401))
    mockGetDaemonConnectionInfo.mockResolvedValue(TEST_PAYLOAD)

    await expect(connectDaemonWs()).rejects.toThrow()

    const leaks = errorSpy.mock.calls.filter(call =>
      call.some(value => typeof value === 'string' && value.includes('tauri-bearer-token'))
    )
    expect(leaks).toHaveLength(0)
    errorSpy.mockRestore()
  })
})
