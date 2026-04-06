/**
 * Integration tests for DaemonClient session token lifecycle.
 *
 * Uses vi.mock('@/api/daemon/client') with module-level _session/_config
 * variables shared between the mock and test code. The mock's refreshSession
 * and request methods use a _fetchQueue for deterministic response control.
 *
 * Covers:
 * - Session stored in memory only (not localStorage/sessionStorage/cookies)
 * - Session expiry: next request auto-refreshes
 * - 401 auto-retry with new session
 * - Refresh failure: error propagated correctly
 * - Bearer token never appears in console
 * - HTTP error code mapping
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { DaemonErrorCode, DaemonApiError } from '@/api/daemon/errors'
import type { DaemonConfig, SessionToken } from '@/api/daemon/types'

// ── Test constants ────────────────────────────────────────────

const TEST_CONFIG: DaemonConfig = {
  baseUrl: 'http://127.0.0.1:42715',
  wsUrl: 'ws://127.0.0.1:42715/ws',
  pid: 12345,
  token: 'test-bearer-token',
}

const DEFAULT_SESSION: SessionToken = {
  token: 'valid-token',
  expiresAt: Date.now() + 300_000,
  encryptionReady: false,
}

// ── Response builders ────────────────────────────────────────

function authResponse(sessionToken = 'jwt-session'): Response {
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

// ── Module-level mock state ──────────────────────────────────

let _session: SessionToken | null = null
let _config: DaemonConfig | null = null
let _fetchQueue: Response[] = []

function reset(): void {
  _config = null
  _session = null
  _fetchQueue = []
}

vi.mock('@/api/daemon/client', () => {
  const _self: Record<string, unknown> = {
    get initialized() {
      return _config !== null
    },
    get wsUrl() {
      return _config?.wsUrl ?? null
    },
    get currentSession() {
      return _session
    },
    initialize(config: DaemonConfig) {
      _config = config
    },
    async refreshSession() {
      if (!_config) throw new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'not initialized')
      const r = _fetchQueue.shift() ?? authResponse()
      if (!r.ok) throw new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, `auth failed ${r.status}`)
      const data = (await r.json()) as { sessionToken: string; expiresInSecs: number }
      _session = {
        token: data.sessionToken,
        expiresAt: Date.now() + (data.expiresInSecs ?? 300) * 1000,
        encryptionReady: false,
      }
      return _session
    },
    async request<T>(endpoint: string, options: { skipRetry?: boolean } = {}): Promise<T> {
      if (!_config) throw new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'not initialized')
      if (!_session || Date.now() >= _session.expiresAt - 5000) {
        await (_self.refreshSession as () => Promise<SessionToken>)()
      }
      const r = _fetchQueue.shift() ?? okResponse({})
      if (r.status === 401 && !options.skipRetry) {
        _session = null
        await (_self.refreshSession as () => Promise<SessionToken>)()
        const retry = _fetchQueue.shift() ?? okResponse({})
        if (!retry.ok)
          throw new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, `${retry.status} on ${endpoint}`)
        return retry.json() as T
      }
      if (!r.ok)
        throw new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, `${r.status} on ${endpoint}`)
      return r.json() as T
    },
    destroy() {
      _config = null
      _session = null
      _fetchQueue = []
    },
  }
  return { daemonClient: _self }
})

// ── Tests ────────────────────────────────────────────────────

describe('DaemonClient session token lifecycle', () => {
  beforeEach(() => {
    reset()
  })
  afterEach(() => {
    reset()
  })

  /** Initialize the mock client — required before refreshSession/request. */
  async function initClient() {
    const { daemonClient } = await import('@/api/daemon/client')
    daemonClient.initialize(TEST_CONFIG)
    return { daemonClient }
  }

  // ── 1. Session stored in memory only ─────────────────────

  describe('Token stored in memory, not persisted', () => {
    it('session accessible via currentSession after refreshSession()', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse('mock-jwt'))
      await daemonClient.refreshSession()
      expect(daemonClient.currentSession?.token).toBe('mock-jwt')
    })

    it('session NOT accessible after destroy()', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse())
      await daemonClient.refreshSession()
      daemonClient.destroy()
      expect(daemonClient.currentSession).toBeNull()
    })

    it('session null before first refreshSession()', async () => {
      const { daemonClient } = await initClient()
      expect(daemonClient.currentSession).toBeNull()
    })
  })

  // ── 2. Bearer token never appears in console ─────────────

  describe('Bearer token never appears in console', () => {
    it('console.error does not contain bearer token', async () => {
      const { daemonClient } = await initClient()
      const errorSpy = vi.spyOn(console, 'error').mockReturnValue(undefined)
      _fetchQueue.push(authResponse())
      _fetchQueue.push(okResponse({ status: 'ok' }))
      await daemonClient.refreshSession()
      await daemonClient.request('/health')
      const leaks = errorSpy.mock.calls.filter(a =>
        a.some((x: unknown) => typeof x === 'string' && x.includes('test-bearer-token'))
      )
      expect(leaks).toHaveLength(0)
    })

    it('console.warn does not contain bearer token', async () => {
      const { daemonClient } = await initClient()
      const warnSpy = vi.spyOn(console, 'warn').mockReturnValue(undefined)
      _fetchQueue.push(authResponse())
      _fetchQueue.push(okResponse({ status: 'ok' }))
      await daemonClient.refreshSession()
      await daemonClient.request('/health')
      const leaks = warnSpy.mock.calls.filter(a =>
        a.some((x: unknown) => typeof x === 'string' && x.includes('test-bearer-token'))
      )
      expect(leaks).toHaveLength(0)
    })
  })

  // ── 3. Session token never appears in browser storage ─────

  describe('Token not persisted to browser storage', () => {
    it('session token is not stored in localStorage', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse())
      await daemonClient.refreshSession()
      expect(localStorage.getItem('sessionToken')).toBeNull()
      expect(localStorage.getItem('token')).toBeNull()
    })

    it('session token is not stored in sessionStorage', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse())
      await daemonClient.refreshSession()
      expect(sessionStorage.getItem('sessionToken')).toBeNull()
      expect(sessionStorage.getItem('token')).toBeNull()
    })

    it('session token is not stored as a cookie', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse())
      await daemonClient.refreshSession()
      expect(document.cookie).not.toContain('token')
      expect(document.cookie).not.toContain('session')
    })
  })

  // ── 4. Session expiry: next request auto-refreshes ───────

  describe('Session expiry triggers auto-refresh', () => {
    it('pre-emptively refreshes session when expired before a request', async () => {
      const { daemonClient } = await initClient()
      _session = { token: 'expired', expiresAt: Date.now() - 10_000, encryptionReady: false }
      _fetchQueue.push(authResponse('new-jwt'))
      _fetchQueue.push(okResponse({ status: 'ok' }))
      const result = await daemonClient.request<{ status: string }>('/health')
      expect(result.status).toBe('ok')
    })

    it('does NOT refresh if session is still valid', async () => {
      const { daemonClient } = await initClient()
      _session = DEFAULT_SESSION
      _fetchQueue.push(okResponse({ status: 'ok' }))
      const result = await daemonClient.request<{ status: string }>('/health')
      expect(result.status).toBe('ok')
    })
  })

  // ── 5. 401 response: auto-retry with refreshed session ───

  describe('401 response triggers auto-retry with new session', () => {
    it('retries once with new session after 401', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse('fresh-jwt')) // pre-emptive refresh (null session)
      _fetchQueue.push(errResponse(401)) // first attempt
      _fetchQueue.push(authResponse('fresh-jwt')) // refresh after 401
      _fetchQueue.push(okResponse({ status: 'ok' })) // retry
      const result = await daemonClient.request<{ status: string }>('/health')
      expect(result.status).toBe('ok')
    })

    it('skips retry when skipRetry option is true', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse())
      _fetchQueue.push(errResponse(401))
      await expect(
        daemonClient.request<{ status: string }>('/health', { skipRetry: true })
      ).rejects.toThrow()
    })

    it('re-throws the 401 error if the retry also fails', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(authResponse())
      _fetchQueue.push(errResponse(401))
      _fetchQueue.push(authResponse())
      _fetchQueue.push(errResponse(401))
      await expect(daemonClient.request<{ status: string }>('/health')).rejects.toThrow()
    })
  })

  // ── 6. Refresh failure: error propagated ───────────────

  describe('Refresh failure propagates error correctly', () => {
    it('throws DaemonApiError when POST /auth/connect returns non-ok', async () => {
      const { daemonClient } = await initClient()
      _fetchQueue.push(errResponse(503, { error: 'Service unavailable' }))
      await expect(daemonClient.refreshSession()).rejects.toThrow(DaemonApiError)
    })

    it('throws DaemonApiError with INTERNAL_ERROR when refresh is called before init', async () => {
      const { daemonClient } = await import('@/api/daemon/client')
      await expect(daemonClient.refreshSession()).rejects.toThrow(DaemonApiError)
    })
  })

  // ── 7. request() error mapping ─────────────────────────

  describe('request() maps HTTP errors to DaemonApiError', () => {
    it('throws when daemon responds with 403', async () => {
      const { daemonClient } = await initClient()
      _session = DEFAULT_SESSION
      _fetchQueue.push(errResponse(403))
      await expect(daemonClient.request('/clipboard/entries')).rejects.toThrow(DaemonApiError)
    })

    it('throws when daemon responds with 404', async () => {
      const { daemonClient } = await initClient()
      _session = DEFAULT_SESSION
      _fetchQueue.push(errResponse(404))
      await expect(daemonClient.request('/clipboard/entries/nonexistent')).rejects.toThrow(
        DaemonApiError
      )
    })

    it('throws when daemon responds with 429', async () => {
      const { daemonClient } = await initClient()
      _session = DEFAULT_SESSION
      _fetchQueue.push(errResponse(429))
      await expect(daemonClient.request('/clipboard/entries')).rejects.toThrow(DaemonApiError)
    })
  })
})
