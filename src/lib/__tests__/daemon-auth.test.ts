/* @vitest-environment jsdom */

import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { getEncryptionState, getHealth } from '@/api/generated/sdk.gen'
import { loadDaemonAuth, verifyAuthState, waitForEncryptionReady } from '@/lib/daemon-auth'
import type { DaemonAuthResult } from '@/lib/daemon-auth'
import { resetDaemonConnectionInfoPollingForTests } from '@/lib/daemon-connection-info'

const mockInvokeWithTrace = vi.fn()

// 实现已切到 typed `commands` proxy（`@/lib/ipc`）；mock target 改成
// `commands.getDaemonConnectionInfo`。变量名保留以减少 diff 噪声。
vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonConnectionInfo: (...args: unknown[]) => mockInvokeWithTrace(...args),
  },
}))

const mockInitialize = vi.fn()
const mockRefreshSession = vi.fn()
const mockDestroy = vi.fn()

// ADR-008 P7: daemon-auth 走生成的 SDK。
// - `/encryption/state` 经 `daemonClient.callSdk`，mock 复刻 callSdk 快乐路径：
//   调用 SDK thunk 并解包其 `{ data }`（= ApiEnvelope）。
// - `/health` 直接调用 `getHealth`（不经 callSdk），所以不受此 mock 影响。
vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    initialize: (...args: unknown[]) => mockInitialize(...args),
    refreshSession: (...args: unknown[]) => mockRefreshSession(...args),
    callSdk: vi.fn((call: () => Promise<{ data: unknown }>) => call().then(r => r.data)),
    destroy: (...args: unknown[]) => mockDestroy(...args),
    get initialized() {
      return true
    },
  },
}))

vi.mock('@/api/generated/sdk.gen', () => ({
  getHealth: vi.fn(),
  getEncryptionState: vi.fn(),
}))

// 类型化的 mock 引用。
const healthMock = getHealth as unknown as ReturnType<typeof vi.fn>
const encryptionStateMock = getEncryptionState as unknown as ReturnType<typeof vi.fn>

const TEST_CONNECTION_PAYLOAD = {
  baseUrl: 'http://127.0.0.1:42715',
  wsUrl: 'ws://127.0.0.1:42715/ws',
}

const TEST_SESSION = {
  token: 'jwt-session-token',
  expiresAt: Date.now() + 300_000,
  encryptionReady: false,
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers()
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
    // ADR-008 P7: GET /health → generated `getHealth` (called directly, resolves
    // to `{ data: <HealthEnvelope> }` where envelope = `{ data: { status }, ts }`).
    // GET /encryption/state → `getEncryptionState` via `callSdk`; the SDK fn
    // resolves to `{ data: <EncryptionStateEnvelope> }` and the callSdk mock
    // unwraps the outer `{ data }` to the envelope.
    healthMock.mockResolvedValueOnce({ data: { data: { status: 'ok' }, ts: 0 } })
    encryptionStateMock.mockResolvedValueOnce({
      data: { data: { initialized: true, sessionReady: true }, ts: 0 },
    })

    const result = await verifyAuthState()

    expect(result.daemonReady).toBe(true)
    expect(result.encryptionInitialized).toBe(true)
    expect(result.encryptionSessionReady).toBe(true)
    expect(healthMock).toHaveBeenCalledWith({ throwOnError: true })
    expect(encryptionStateMock).toHaveBeenCalledWith({ throwOnError: true })
  })

  it('returns all-false when daemon is unreachable', async () => {
    healthMock.mockRejectedValueOnce(new Error('connection refused'))

    const result = await verifyAuthState()

    expect(result.daemonReady).toBe(false)
    expect(result.encryptionInitialized).toBe(false)
    expect(result.encryptionSessionReady).toBe(false)
    expect(healthMock).toHaveBeenCalledTimes(1)
    expect(encryptionStateMock).not.toHaveBeenCalled()
  })

  it('returns daemonReady=true but encryption=false when encryption check fails', async () => {
    healthMock.mockResolvedValueOnce({ data: { data: { status: 'ok' }, ts: 0 } })
    encryptionStateMock.mockRejectedValueOnce(
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
    encryptionStateMock.mockResolvedValueOnce({
      data: { data: { initialized: true, sessionReady: true }, ts: 0 },
    })

    const result = await waitForEncryptionReady(5000)

    expect(result).toBe(true)
    expect(encryptionStateMock).toHaveBeenCalledTimes(1)
    expect(encryptionStateMock).toHaveBeenCalledWith({ throwOnError: true })
  })

  it('resolves false on timeout', async () => {
    encryptionStateMock.mockResolvedValue({
      data: { data: { initialized: true, sessionReady: false }, ts: 0 },
    })

    const promise = waitForEncryptionReady(1500)
    await vi.advanceTimersByTimeAsync(2000)

    await expect(promise).resolves.toBe(false)
  })
})
