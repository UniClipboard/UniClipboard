/* @vitest-environment jsdom */

import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { loadDaemonAuth, verifyAuthState, waitForEncryptionReady } from '@/lib/daemon-auth'
import type { DaemonAuthResult } from '@/lib/daemon-auth'
import { resetDaemonConnectionInfoPollingForTests } from '@/lib/daemon-connection-info'

const mockInvoke = vi.fn()
const mockInvokeWithTrace = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}))

// 实现已切到 typed `commands` proxy（`@/lib/ipc`）；mock target 改成
// `commands.getDaemonConnectionInfo`。变量名保留以减少 diff 噪声。
vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonConnectionInfo: (...args: unknown[]) => mockInvokeWithTrace(...args),
  },
}))

const mockInitialize = vi.fn()
const mockRefreshSession = vi.fn()
const mockRequest = vi.fn()
const mockDestroy = vi.fn()

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    initialize: (...args: unknown[]) => mockInitialize(...args),
    refreshSession: (...args: unknown[]) => mockRefreshSession(...args),
    request: (...args: unknown[]) => mockRequest(...args),
    destroy: (...args: unknown[]) => mockDestroy(...args),
    get initialized() {
      return true
    },
  },
}))

const TEST_CONNECTION_PAYLOAD = {
  baseUrl: 'http://127.0.0.1:42715',
  wsUrl: 'ws://127.0.0.1:42715/ws',
  token: 'test-bearer-token',
}

const TEST_SESSION = {
  token: 'jwt-session-token',
  expiresAt: Date.now() + 300_000,
  encryptionReady: false,
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers()
  mockInvoke.mockResolvedValue(4242)
  resetDaemonConnectionInfoPollingForTests()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('loadDaemonAuth', () => {
  it('polls get_daemon_connection_info until payload is available and initializes the client', async () => {
    mockInvokeWithTrace.mockResolvedValueOnce(null).mockResolvedValueOnce(TEST_CONNECTION_PAYLOAD)
    mockRefreshSession.mockResolvedValue(TEST_SESSION)

    const resultPromise: Promise<DaemonAuthResult> = loadDaemonAuth()
    await vi.advanceTimersByTimeAsync(500)
    const result = await resultPromise

    expect(mockInvokeWithTrace).toHaveBeenNthCalledWith(1)
    expect(mockInvokeWithTrace).toHaveBeenNthCalledWith(2)
    expect(mockInitialize).toHaveBeenCalledWith({
      baseUrl: TEST_CONNECTION_PAYLOAD.baseUrl,
      wsUrl: TEST_CONNECTION_PAYLOAD.wsUrl,
      token: TEST_CONNECTION_PAYLOAD.token,
      pid: 4242,
    })
    expect(mockRefreshSession).toHaveBeenCalledOnce()
    expect(result.session).toEqual(TEST_SESSION)
    expect(result.wsUrl).toBe(TEST_CONNECTION_PAYLOAD.wsUrl)
  })

  it('propagates refreshSession errors after polling succeeds', async () => {
    mockInvokeWithTrace.mockResolvedValue(TEST_CONNECTION_PAYLOAD)
    mockRefreshSession.mockRejectedValue(
      new DaemonApiError(DaemonErrorCode.UNAUTHORIZED, 'bad token')
    )

    await expect(loadDaemonAuth()).rejects.toThrow('bad token')
    expect(mockInitialize).toHaveBeenCalledOnce()
  })
})

describe('verifyAuthState', () => {
  it('returns full state when daemon is healthy and encryption initialized', async () => {
    mockRequest.mockResolvedValueOnce({ status: 'ok' })
    mockRequest.mockResolvedValueOnce({
      data: { initialized: true, sessionReady: true },
      ts: Date.now(),
    })

    const result = await verifyAuthState()

    expect(result.daemonReady).toBe(true)
    expect(result.encryptionInitialized).toBe(true)
    expect(result.encryptionSessionReady).toBe(true)
    expect(mockRequest).toHaveBeenNthCalledWith(1, '/health')
    expect(mockRequest).toHaveBeenNthCalledWith(2, '/encryption/state')
  })

  it('returns all-false when daemon is unreachable', async () => {
    mockRequest.mockRejectedValueOnce(new Error('connection refused'))

    const result = await verifyAuthState()

    expect(result.daemonReady).toBe(false)
    expect(result.encryptionInitialized).toBe(false)
    expect(result.encryptionSessionReady).toBe(false)
    expect(mockRequest).toHaveBeenCalledTimes(1)
  })

  it('returns daemonReady=true but encryption=false when encryption check fails', async () => {
    mockRequest.mockResolvedValueOnce({ status: 'ok' })
    mockRequest.mockRejectedValueOnce(
      new DaemonApiError(DaemonErrorCode.UNAUTHORIZED, 'session expired')
    )

    const result = await verifyAuthState()

    expect(result.daemonReady).toBe(true)
    expect(result.encryptionInitialized).toBe(false)
    expect(result.encryptionSessionReady).toBe(false)
  })
})

describe('waitForEncryptionReady', () => {
  it('resolves true immediately when encryption is ready on first poll', async () => {
    mockRequest.mockResolvedValueOnce({
      data: { initialized: true, sessionReady: true },
      ts: Date.now(),
    })

    const result = await waitForEncryptionReady(5000)

    expect(result).toBe(true)
    expect(mockRequest).toHaveBeenCalledTimes(1)
    expect(mockRequest).toHaveBeenCalledWith('/encryption/state')
  })

  it('resolves false on timeout', async () => {
    mockRequest.mockResolvedValue({
      data: { initialized: true, sessionReady: false },
      ts: Date.now(),
    })

    const promise = waitForEncryptionReady(1500)
    await vi.advanceTimersByTimeAsync(2000)

    await expect(promise).resolves.toBe(false)
  })
})
