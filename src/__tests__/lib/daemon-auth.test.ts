/**
 * Integration tests for daemon-auth module (loadDaemonAuth, verifyAuthState, waitForEncryptionReady).
 *
 * Covers:
 * - loadDaemonAuth(): command polling → DaemonClient init → session token obtained, stored in memory
 * - verifyAuthState(): daemon health + encryption state checking
 * - waitForEncryptionReady(): timeout and session-ready polling
 * - Token stored in memory, not localStorage/sessionStorage/cookies
 * - Bearer token never appears in console
 *
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SessionToken } from '@/api/daemon/types'
import { resetDaemonConnectionInfoPollingForTests } from '@/lib/daemon-connection-info'

const mockInvoke = vi.fn()
const mockInvokeWithTrace = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}))

// 实现已切到 typed `commands` proxy（`@/lib/ipc`）；mock target 改成
// `commands.getDaemonConnectionInfo`，变量名保留减少 diff 噪声。
vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonConnectionInfo: (...args: unknown[]) => mockInvokeWithTrace(...args),
  },
}))

let _session: SessionToken | null = null
let _fetchQueue: Response[] = []

const TEST_PAYLOAD = {
  baseUrl: 'http://127.0.0.1:42715',
  wsUrl: 'ws://127.0.0.1:42715/ws',
}

function reset(): void {
  _session = null
  _fetchQueue = []
  mockInvoke.mockReset()
  mockInvokeWithTrace.mockReset()
}

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    get initialized() {
      return true
    },
    get wsUrl() {
      return TEST_PAYLOAD.wsUrl
    },
    get currentSession() {
      return _session
    },
    initialize(_config: unknown) {},
    async refreshSession() {
      const response = _fetchQueue.shift() ?? authResponse('mock-jwt-session')
      if (!response.ok) throw new Error(`auth failed ${response.status}`)
      const data = (await response.json()) as { sessionToken: string; expiresInSecs: number }
      _session = {
        token: data.sessionToken,
        expiresAt: Date.now() + data.expiresInSecs * 1000,
        encryptionReady: false,
      }
      return _session
    },
    async request<T>(endpoint: string): Promise<T> {
      const response = _fetchQueue.shift() ?? okResponse({})
      if (!response.ok) {
        throw Object.assign(new Error(`${response.status} on ${endpoint}`), {
          code: 'INTERNAL_ERROR',
        })
      }
      return response.json() as Promise<T>
    },
    destroy() {
      _session = null
      _fetchQueue = []
    },
  },
}))

function authResponse(sessionToken = 'mock-jwt-session'): Response {
  return new Response(JSON.stringify({ sessionToken, expiresInSecs: 300, refreshAtSecs: 240 }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function okResponse(body: object): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function errResponse(status: number, body?: unknown): Response {
  return new Response(JSON.stringify(body ?? { error: `HTTP ${status}` }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('daemon-auth module', () => {
  beforeEach(() => {
    reset()
    resetDaemonConnectionInfoPollingForTests()
    vi.useFakeTimers()
    mockInvoke.mockResolvedValue(4242)
  })

  afterEach(() => {
    reset()
    resetDaemonConnectionInfoPollingForTests()
    vi.useRealTimers()
  })

  describe('loadDaemonAuth()', () => {
    it('initializes DaemonClient with connection info from command polling', async () => {
      const { loadDaemonAuth } = await import('@/lib/daemon-auth')
      _fetchQueue.push(authResponse())
      mockInvokeWithTrace.mockResolvedValueOnce(null).mockResolvedValueOnce(TEST_PAYLOAD)

      const promise = loadDaemonAuth()
      await vi.advanceTimersByTimeAsync(500)
      const result = await promise

      expect(mockInvokeWithTrace).toHaveBeenNthCalledWith(1)
      expect(mockInvokeWithTrace).toHaveBeenNthCalledWith(2)
      expect(result.session).not.toBeNull()
      expect(result.session.token).toBe('mock-jwt-session')
      expect(result.wsUrl).toBe(TEST_PAYLOAD.wsUrl)
    })

    it('session token is stored in daemonClient.currentSession, not external storage', async () => {
      const { loadDaemonAuth } = await import('@/lib/daemon-auth')
      _fetchQueue.push(authResponse())
      mockInvokeWithTrace.mockResolvedValue(TEST_PAYLOAD)

      await loadDaemonAuth()

      expect(_session?.token).toBe('mock-jwt-session')
      expect(localStorage.getItem('sessionToken')).toBeNull()
      expect(sessionStorage.getItem('sessionToken')).toBeNull()
      expect(document.cookie).not.toContain('jwt-session')
    })

    it('promise stays pending while command keeps returning null', async () => {
      const { loadDaemonAuth } = await import('@/lib/daemon-auth')
      mockInvokeWithTrace.mockResolvedValue(null)

      const promise = loadDaemonAuth()
      let settled = false
      void promise.then(() => {
        settled = true
      })

      await vi.advanceTimersByTimeAsync(2000)

      expect(settled).toBe(false)
      expect(mockInvokeWithTrace).toHaveBeenCalled()
    })

    it('bearer token never appears in console.error or console.warn', async () => {
      const { loadDaemonAuth } = await import('@/lib/daemon-auth')
      const errorSpy = vi.spyOn(console, 'error').mockReturnValue(undefined)
      const warnSpy = vi.spyOn(console, 'warn').mockReturnValue(undefined)
      _fetchQueue.push(authResponse())
      mockInvokeWithTrace.mockResolvedValue(TEST_PAYLOAD)

      await loadDaemonAuth()

      const leaks = (spy: ReturnType<typeof vi.spyOn>) =>
        spy.mock.calls.filter((call: unknown[]) =>
          call.some(
            (value: unknown) => typeof value === 'string' && value.includes('tauri-bearer-token')
          )
        )

      expect(leaks(errorSpy)).toHaveLength(0)
      expect(leaks(warnSpy)).toHaveLength(0)
    })
  })

  describe('verifyAuthState()', () => {
    it('returns daemonReady=true when GET /health returns ok', async () => {
      const { verifyAuthState } = await import('@/lib/daemon-auth')
      _fetchQueue.push(okResponse({ status: 'ok' }))
      _fetchQueue.push(errResponse(401))

      const result = await verifyAuthState()
      expect(result.daemonReady).toBe(true)
    })

    it('returns daemonReady=false when GET /health fails', async () => {
      const { verifyAuthState } = await import('@/lib/daemon-auth')
      _fetchQueue.push(errResponse(500))

      const result = await verifyAuthState()
      expect(result.daemonReady).toBe(false)
    })
  })

  describe('waitForEncryptionReady()', () => {
    it('returns true when encryption state reaches sessionReady=true', async () => {
      const { waitForEncryptionReady } = await import('@/lib/daemon-auth')
      _fetchQueue.push(
        okResponse({ data: { initialized: true, sessionReady: true }, ts: 1710000000000 })
      )

      await expect(waitForEncryptionReady(5000)).resolves.toBe(true)
    })

    it('returns false when timeout expires before sessionReady=true', async () => {
      const { waitForEncryptionReady } = await import('@/lib/daemon-auth')
      _fetchQueue.push(
        okResponse({ data: { initialized: true, sessionReady: false }, ts: 1710000000000 })
      )

      const promise = waitForEncryptionReady(3000)
      await vi.advanceTimersByTimeAsync(3000)

      await expect(promise).resolves.toBe(false)
    })
  })
})
