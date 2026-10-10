import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import type { DaemonConfig } from '@/api/daemon/types'

const mockGetDaemonSession = vi.fn()

vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonSession: (...args: unknown[]) => mockGetDaemonSession(...args),
  },
}))

const TEST_CONFIG: DaemonConfig = {
  baseUrl: 'http://127.0.0.1:9999',
  wsUrl: 'ws://127.0.0.1:9999/ws',
}

describe('DaemonClient empty success responses', () => {
  beforeEach(() => {
    daemonClient.destroy()
    mockGetDaemonSession.mockResolvedValue({
      sessionToken: 'jwt-empty',
      expiresInSecs: 300,
      refreshAtSecs: 240,
    })
  })

  afterEach(() => {
    daemonClient.destroy()
    mockGetDaemonSession.mockReset()
    vi.unstubAllGlobals()
  })

  it('treats 200 OK with an empty body as success for void requests', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(new Response(null, { status: 200 })))

    daemonClient.initialize(TEST_CONFIG)

    await expect(
      daemonClient.request<void>('/pairing/accept', {
        method: 'POST',
        body: { sessionId: 'session-xyz' },
      })
    ).resolves.toBeUndefined()
  })

  it('still treats 204 No Content as success for void requests', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(new Response(null, { status: 204 })))

    daemonClient.initialize(TEST_CONFIG)

    await expect(
      daemonClient.request<void>('/pairing/reject', {
        method: 'POST',
        body: { sessionId: 'session-xyz' },
      })
    ).resolves.toBeUndefined()
  })
})
