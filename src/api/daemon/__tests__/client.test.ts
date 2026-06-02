import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { SdkRequestError } from '@/api/daemon/generated-bridge'
import type { DaemonConfig } from '@/api/daemon/types'

const mockGetDaemonSession = vi.fn()

vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonSession: (...args: unknown[]) => mockGetDaemonSession(...args),
  },
}))

// ── Test helpers ────────────────────────────────────────────────

const TEST_CONFIG: DaemonConfig = {
  baseUrl: 'http://127.0.0.1:9999',
  wsUrl: 'ws://127.0.0.1:9999/ws',
}

const TEST_SESSION_PAYLOAD = {
  sessionToken: 'jwt-abc',
  expiresInSecs: 300,
  refreshAtSecs: 240,
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

// ── Tests ───────────────────────────────────────────────────────

describe('DaemonClient', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    // Reset the singleton between tests by destroying any prior state.
    daemonClient.destroy()
    mockGetDaemonSession.mockResolvedValue(TEST_SESSION_PAYLOAD)
  })

  afterEach(() => {
    daemonClient.destroy()
    mockGetDaemonSession.mockReset()
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

    it('gets a native-managed session and returns it', async () => {
      daemonClient.initialize(TEST_CONFIG)
      const session = await daemonClient.refreshSession()

      expect(session.token).toBe('jwt-abc')
      expect(session.expiresAt).toBeGreaterThan(Date.now())
      expect(mockGetDaemonSession).toHaveBeenCalledOnce()
    })

    it('coalesces concurrent refresh calls', async () => {
      daemonClient.initialize(TEST_CONFIG)

      const [s1, s2] = await Promise.all([
        daemonClient.refreshSession(),
        daemonClient.refreshSession(),
      ])

      expect(s1).toBe(s2)
      expect(mockGetDaemonSession).toHaveBeenCalledTimes(1)
    })

    it('throws DaemonApiError when native session is unavailable', async () => {
      mockGetDaemonSession.mockResolvedValueOnce(null)

      daemonClient.initialize(TEST_CONFIG)
      const promise = daemonClient.refreshSession()
      await expect(promise).rejects.toThrow(DaemonApiError)
      await expect(promise).rejects.toMatchObject({
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
        // actual GET /settings response
        { ok: true, status: 200, body: { theme: 'dark' } },
      ])
      mockGetDaemonSession.mockResolvedValueOnce({
        sessionToken: 'jwt-new',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      const result = await daemonClient.request<{ theme: string }>('/settings')

      expect(result.theme).toBe('dark')

      const settingsCall = vi.mocked(fetch).mock.calls[0]
      expect(settingsCall[0]).toBeInstanceOf(URL)
      expect(settingsCall[0]?.toString()).toBe(
        'http://127.0.0.1:9999/settings?auth=Session+jwt-new'
      )
    })

    it('auto-retries once on 401', async () => {
      mockFetchSequence([
        // first request → 401
        { ok: false, status: 401, body: { error: 'unauthorized' } },
        // retry request → success
        { ok: true, status: 200, body: { data: 'ok' } },
      ])
      mockGetDaemonSession
        .mockResolvedValueOnce({
          sessionToken: 'jwt-old',
          expiresInSecs: 300,
          refreshAtSecs: 240,
        })
        .mockResolvedValueOnce({
          sessionToken: 'jwt-fresh',
          expiresInSecs: 300,
          refreshAtSecs: 240,
        })

      daemonClient.initialize(TEST_CONFIG)
      const result = await daemonClient.request<{ data: string }>('/endpoint')

      expect(result.data).toBe('ok')
      expect(vi.mocked(fetch)).toHaveBeenCalledTimes(2)
      expect(mockGetDaemonSession).toHaveBeenCalledTimes(2)
    })

    it('returns typed response', async () => {
      mockFetchSequence([{ ok: true, status: 200, body: { count: 42, items: ['a', 'b'] } }])
      mockGetDaemonSession.mockResolvedValueOnce({
        sessionToken: 'jwt-t',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      const result = await daemonClient.request<{ count: number; items: string[] }>('/items')

      expect(result.count).toBe(42)
      expect(result.items).toEqual(['a', 'b'])
    })

    it('throws DaemonApiError with mapped error code on non-401 failure', async () => {
      mockFetchSequence([{ ok: false, status: 404, body: { error: 'not found' } }])
      mockGetDaemonSession.mockResolvedValueOnce({
        sessionToken: 'jwt-t',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

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

  // ── callSdk<T> ──────────────────────────────────────────────

  describe('callSdk<T>', () => {
    /** Minimal Response stand-in — normalizeSdkError only reads status + url. */
    function fakeResponse(status: number, url: string): Response {
      return { status, url } as unknown as Response
    }

    it('unwraps { data } on the happy path', async () => {
      daemonClient.initialize(TEST_CONFIG)
      await daemonClient.refreshSession()

      const result = await daemonClient.callSdk(() => Promise.resolve({ data: { theme: 'dark' } }))
      expect(result).toEqual({ theme: 'dark' })
    })

    it('normalizes a non-401 SdkRequestError to DaemonApiError (status→code, body→details)', async () => {
      daemonClient.initialize(TEST_CONFIG)
      await daemonClient.refreshSession()

      // 410 Gone — the restore PAYLOAD_UNAVAILABLE path. The normalized server
      // body carries the lost-payload context under `.details`.
      const body = {
        code: 'PAYLOAD_UNAVAILABLE',
        message: 'content lost',
        details: { entryId: 'e1' },
      }
      const promise = daemonClient.callSdk(() =>
        Promise.reject(
          new SdkRequestError(
            body,
            fakeResponse(410, 'http://127.0.0.1:9999/clipboard/restore/e1?plain=true')
          )
        )
      )

      await expect(promise).rejects.toBeInstanceOf(DaemonApiError)
      await expect(promise).rejects.toMatchObject({
        code: DaemonErrorCode.PAYLOAD_UNAVAILABLE,
        // `.details` is the full normalized body → `.details.message` /
        // `.details.details` stay reachable for downstream classifiers.
        details: body,
      })
      // setupV2 classifiers regex the status out of the message prefix.
      await expect(promise).rejects.toMatchObject({
        message: expect.stringMatching(/^410 on \/clipboard\/restore\/e1/),
      })
    })

    it('refreshes once and retries on 401, then succeeds', async () => {
      daemonClient.initialize(TEST_CONFIG)
      await daemonClient.refreshSession()
      mockGetDaemonSession.mockClear()

      let attempts = 0
      const result = await daemonClient.callSdk(() => {
        attempts += 1
        if (attempts === 1) {
          return Promise.reject(
            new SdkRequestError(
              { code: 'UNAUTHORIZED' },
              fakeResponse(401, 'http://127.0.0.1:9999/settings')
            )
          )
        }
        return Promise.resolve({ data: { ok: true } })
      })

      expect(result).toEqual({ ok: true })
      expect(attempts).toBe(2)
      expect(mockGetDaemonSession).toHaveBeenCalledTimes(1)
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
      mockGetDaemonSession.mockResolvedValue({
        sessionToken: 'jwt-alive',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)

      // Advance 240 seconds → should trigger one refresh
      await vi.advanceTimersByTimeAsync(240_000)
      expect(mockGetDaemonSession).toHaveBeenCalledTimes(1)

      // Advance another 240 seconds → second refresh
      await vi.advanceTimersByTimeAsync(240_000)
      expect(mockGetDaemonSession).toHaveBeenCalledTimes(2)
    })

    it('stops after destroy', async () => {
      mockGetDaemonSession.mockResolvedValue({
        sessionToken: 'jwt-alive',
        expiresInSecs: 300,
        refreshAtSecs: 240,
      })

      daemonClient.initialize(TEST_CONFIG)
      daemonClient.destroy()

      await vi.advanceTimersByTimeAsync(240_000)
      expect(mockGetDaemonSession).toHaveBeenCalledTimes(0)
    })
  })
})
