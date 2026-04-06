import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import type { DaemonConfig } from '@/api/daemon/types'

// ── Test helpers ────────────────────────────────────────────────

const TEST_CONFIG: DaemonConfig = {
  baseUrl: 'http://127.0.0.1:9999',
  wsUrl: 'ws://127.0.0.1:9999/ws',
  pid: 12345,
  token: 'test-bearer-token',
}

function mockFetchOk(body: unknown, status = 200): void {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: true,
      status,
      json: () => Promise.resolve(body),
      text: () => Promise.resolve(JSON.stringify(body)),
    })
  )
}

function mockFetchSequence(responses: Array<{ ok: boolean; status: number; body: unknown }>): void {
  const fetchMock = vi.fn()
  for (const [, resp] of responses.entries()) {
    fetchMock.mockResolvedValueOnce({
      ok: resp.ok,
      status: resp.status,
      json: () => Promise.resolve(resp.body),
      text: () => Promise.resolve(JSON.stringify(resp.body)),
    })
  }
  vi.stubGlobal('fetch', fetchMock)
}

function mockFetchError(status: number, body: unknown = { error: 'fail' }): void {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: false,
      status,
      json: () => Promise.resolve(body),
      text: () => Promise.resolve(JSON.stringify(body)),
    })
  )
}

// ── Tests ───────────────────────────────────────────────────────

describe('DaemonClient', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    // Reset the singleton between tests by destroying any prior state.
    daemonClient.destroy()
  })

  afterEach(() => {
    daemonClient.destroy()
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  // ── initialize ──────────────────────────────────────────────

  describe('initialize', () => {
    it('sets initialized to true and stores wsUrl', () => {
      expect(daemonClient.initialized).toBe(false)
      daemonClient.initialize(TEST_CONFIG)
      expect(daemonClient.initialized).toBe(true)
      expect(daemonClient.wsUrl).toBe('ws://127.0.0.1:9999/ws')
    })
  })

  describe('blobUrl', () => {
    it('returns non-daemon urls unchanged', async () => {
      mockFetchOk({
        sessionToken: 'jwt-abc',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      await daemonClient.refreshSession()

      expect(daemonClient.blobUrl('data:image/png;base64,abc')).toBe('data:image/png;base64,abc')
      expect(daemonClient.blobUrl('blob:http://localhost/preview-1')).toBe(
        'blob:http://localhost/preview-1'
      )
      expect(daemonClient.blobUrl('https://cdn.example.com/image.png')).toBe(
        'https://cdn.example.com/image.png'
      )
      expect(daemonClient.blobUrl('http://cdn.example.com/image.png')).toBe(
        'http://cdn.example.com/image.png'
      )
    })

    it('adds auth only for relative daemon resource paths', async () => {
      mockFetchOk({
        sessionToken: 'jwt-abc',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      await daemonClient.refreshSession()

      expect(daemonClient.blobUrl('/clipboard/blobs/blob-1')).toBe(
        'http://127.0.0.1:9999/clipboard/blobs/blob-1?auth=Session+jwt-abc'
      )
    })
  })

  // ── refreshSession ──────────────────────────────────────────

  describe('refreshSession', () => {
    it('throws if not initialized', async () => {
      await expect(daemonClient.refreshSession()).rejects.toThrow(DaemonApiError)
    })

    it('POSTs to /auth/connect with bearer token and returns session', async () => {
      mockFetchOk({
        sessionToken: 'jwt-abc',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      const session = await daemonClient.refreshSession()

      expect(session.token).toBe('jwt-abc')
      expect(session.expiresAt).toBeGreaterThan(Date.now())

      const fetchCall = vi.mocked(fetch).mock.calls[0]
      expect(fetchCall[0]).toBe('http://127.0.0.1:9999/auth/connect')
      expect(fetchCall[1]?.method).toBe('POST')
      expect(fetchCall[1]?.body).toEqual(
        new URLSearchParams({
          token: 'test-bearer-token',
          pid: '12345',
          clientType: 'gui',
        })
      )
    })

    it('coalesces concurrent refresh calls', async () => {
      mockFetchOk({
        sessionToken: 'jwt-abc',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)

      const [s1, s2] = await Promise.all([
        daemonClient.refreshSession(),
        daemonClient.refreshSession(),
      ])

      expect(s1).toBe(s2)
      expect(vi.mocked(fetch)).toHaveBeenCalledTimes(1)
    })

    it('throws DaemonApiError on auth failure', async () => {
      mockFetchError(401, { error: 'unauthorized' })

      daemonClient.initialize(TEST_CONFIG)
      await expect(daemonClient.refreshSession()).rejects.toThrow(DaemonApiError)
      await expect(daemonClient.refreshSession()).rejects.toMatchObject({
        code: DaemonErrorCode.UNAUTHORIZED,
      })
    })
  })

  // ── request<T> ──────────────────────────────────────────────

  describe('request<T>', () => {
    it('throws if not initialized', async () => {
      await expect(daemonClient.request('/settings')).rejects.toThrow(DaemonApiError)
    })

    it('auto-refreshes session and sends request with session header', async () => {
      mockFetchSequence([
        // refreshSession response
        {
          ok: true,
          status: 200,
          body: { sessionToken: 'jwt-new', expiresInSecs: 300, refreshAtSecs: 240 },
        },
        // actual GET /settings response
        { ok: true, status: 200, body: { theme: 'dark' } },
      ])

      daemonClient.initialize(TEST_CONFIG)
      const result = await daemonClient.request<{ theme: string }>('/settings')

      expect(result.theme).toBe('dark')

      const settingsCall = vi.mocked(fetch).mock.calls[1]
      expect(settingsCall[0]).toBeInstanceOf(URL)
      expect(settingsCall[0]?.toString()).toBe(
        'http://127.0.0.1:9999/settings?auth=Session+jwt-new'
      )
    })

    it('auto-retries once on 401', async () => {
      mockFetchSequence([
        // initial refreshSession (pre-request)
        {
          ok: true,
          status: 200,
          body: { sessionToken: 'jwt-old', expiresInSecs: 300, refreshAtSecs: 240 },
        },
        // first request → 401
        { ok: false, status: 401, body: { error: 'unauthorized' } },
        // retry refreshSession
        {
          ok: true,
          status: 200,
          body: { sessionToken: 'jwt-fresh', expiresInSecs: 300, refreshAtSecs: 240 },
        },
        // retry request → success
        { ok: true, status: 200, body: { data: 'ok' } },
      ])

      daemonClient.initialize(TEST_CONFIG)
      const result = await daemonClient.request<{ data: string }>('/endpoint')

      expect(result.data).toBe('ok')
      // 4 fetch calls: refresh + request + re-refresh + retry-request
      expect(vi.mocked(fetch)).toHaveBeenCalledTimes(4)
    })

    it('returns typed response', async () => {
      mockFetchSequence([
        {
          ok: true,
          status: 200,
          body: { sessionToken: 'jwt-t', expiresInSecs: 300, refreshAtSecs: 240 },
        },
        { ok: true, status: 200, body: { count: 42, items: ['a', 'b'] } },
      ])

      daemonClient.initialize(TEST_CONFIG)
      const result = await daemonClient.request<{ count: number; items: string[] }>('/items')

      expect(result.count).toBe(42)
      expect(result.items).toEqual(['a', 'b'])
    })

    it('throws DaemonApiError with mapped error code on non-401 failure', async () => {
      mockFetchSequence([
        {
          ok: true,
          status: 200,
          body: { sessionToken: 'jwt-t', expiresInSecs: 300, refreshAtSecs: 240 },
        },
        { ok: false, status: 404, body: { error: 'not found' } },
      ])

      daemonClient.initialize(TEST_CONFIG)
      try {
        await daemonClient.request('/missing')
        expect.unreachable('should have thrown')
      } catch (err) {
        expect(err).toBeInstanceOf(DaemonApiError)
        expect((err as DaemonApiError).code).toBe(DaemonErrorCode.NOT_FOUND)
      }
    })
  })

  // ── destroy ─────────────────────────────────────────────────

  describe('destroy', () => {
    it('clears session and timer', () => {
      daemonClient.initialize(TEST_CONFIG)
      expect(daemonClient.initialized).toBe(true)

      daemonClient.destroy()
      expect(daemonClient.initialized).toBe(false)
      expect(daemonClient.currentSession).toBeNull()
    })

    it('subsequent requests throw after destroy', async () => {
      daemonClient.initialize(TEST_CONFIG)
      daemonClient.destroy()
      await expect(daemonClient.request('/anything')).rejects.toThrow(DaemonApiError)
    })
  })

  // ── keep-alive timer ────────────────────────────────────────

  describe('keep-alive', () => {
    it('calls refreshSession every 240 seconds', async () => {
      mockFetchOk({
        sessionToken: 'jwt-alive',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)

      // Advance 240 seconds → should trigger one refresh
      await vi.advanceTimersByTimeAsync(240_000)
      expect(vi.mocked(fetch)).toHaveBeenCalledTimes(1)

      // Advance another 240 seconds → second refresh
      await vi.advanceTimersByTimeAsync(240_000)
      expect(vi.mocked(fetch)).toHaveBeenCalledTimes(2)
    })

    it('stops after destroy', async () => {
      mockFetchOk({
        sessionToken: 'jwt-alive',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      daemonClient.destroy()

      await vi.advanceTimersByTimeAsync(240_000)
      expect(vi.mocked(fetch)).toHaveBeenCalledTimes(0)
    })
  })
})
