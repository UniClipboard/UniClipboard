/**
 * daemon-connection-info polling: timeout, retry, and fail-fast behaviour.
 *
 * Guards the fix for the "main window stuck loading forever" bug — when the
 * native daemon bootstrap never populates the connection state (e.g.
 * `RefusedNewerDaemon`, spawn failure, health-check timeout), the poll must
 * either fail fast on a recorded bootstrap failure or eventually time out,
 * instead of looping indefinitely.
 *
 * @vitest-environment node
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  DaemonBootstrapFailedError,
  DaemonConnectionInfoTimeoutError,
  resetDaemonConnectionInfoPollingForTests,
  waitForDaemonConnectionInfo,
} from '@/lib/daemon-connection-info'

const TEST_PAYLOAD = {
  baseUrl: 'http://127.0.0.1:42715',
  wsUrl: 'ws://127.0.0.1:42715/ws',
}

const VERSION_FAILURE = {
  kind: 'versionTooOld' as const,
  detail: 'running daemon 0.15.0 is newer than this client 0.14.0',
  observedVersion: '0.15.0',
  expectedVersion: '0.14.0',
}

// Matches CONNECTION_INFO_TIMEOUT_MS in the module under test (kept local so
// the test fails loudly if the production ceiling changes without review).
const TIMEOUT_MS = 60_000

const mockGetDaemonConnectionInfo = vi.fn()
const mockGetDaemonBootstrapFailure = vi.fn()

vi.mock('@/lib/ipc', () => ({
  commands: {
    getDaemonConnectionInfo: (...args: unknown[]) => mockGetDaemonConnectionInfo(...args),
    getDaemonBootstrapFailure: (...args: unknown[]) => mockGetDaemonBootstrapFailure(...args),
  },
}))

describe('waitForDaemonConnectionInfo()', () => {
  beforeEach(() => {
    resetDaemonConnectionInfoPollingForTests()
    mockGetDaemonConnectionInfo.mockReset()
    mockGetDaemonBootstrapFailure.mockReset()
    // Default: bootstrap has not failed — the common path.
    mockGetDaemonBootstrapFailure.mockResolvedValue(null)
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns the payload once the daemon reports ready', async () => {
    mockGetDaemonConnectionInfo.mockResolvedValueOnce(null).mockResolvedValueOnce(TEST_PAYLOAD)

    const promise = waitForDaemonConnectionInfo()
    await vi.advanceTimersByTimeAsync(500)

    await expect(promise).resolves.toEqual(TEST_PAYLOAD)
  })

  it('fails fast with the typed failure when the native bootstrap reports a terminal error', async () => {
    mockGetDaemonConnectionInfo.mockResolvedValue(null)
    mockGetDaemonBootstrapFailure.mockResolvedValue(VERSION_FAILURE)

    const promise = waitForDaemonConnectionInfo()
    const expectation = expect(promise).rejects.toBeInstanceOf(DaemonBootstrapFailedError)
    // First poll: no connection info, but a recorded failure → throw
    // immediately, no timer/timeout involved.
    await vi.advanceTimersByTimeAsync(0)
    await expectation
  })

  it('carries the typed failure payload on the thrown error', async () => {
    mockGetDaemonConnectionInfo.mockResolvedValue(null)
    mockGetDaemonBootstrapFailure.mockResolvedValue(VERSION_FAILURE)

    const promise = waitForDaemonConnectionInfo()
    const expectation = expect(promise).rejects.toMatchObject({
      name: 'DaemonBootstrapFailedError',
      failure: VERSION_FAILURE,
    })
    await vi.advanceTimersByTimeAsync(0)
    await expectation
  })

  it('rejects with a timeout error when the daemon never becomes reachable', async () => {
    mockGetDaemonConnectionInfo.mockResolvedValue(null)

    const promise = waitForDaemonConnectionInfo()
    const expectation = expect(promise).rejects.toBeInstanceOf(DaemonConnectionInfoTimeoutError)

    // Advance past the ceiling (+ one poll interval of slack).
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS + 500)

    await expectation
  })

  it('clears the cached promise on timeout so a later call can retry and succeed', async () => {
    mockGetDaemonConnectionInfo.mockResolvedValue(null)

    const first = waitForDaemonConnectionInfo()
    const firstExpectation = expect(first).rejects.toBeInstanceOf(DaemonConnectionInfoTimeoutError)
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS + 500)
    await firstExpectation

    // Daemon now reachable — a fresh call must start a new polling sequence
    // rather than re-throwing the cached timeout rejection.
    mockGetDaemonConnectionInfo.mockReset()
    mockGetDaemonConnectionInfo.mockResolvedValueOnce(TEST_PAYLOAD)

    await expect(waitForDaemonConnectionInfo()).resolves.toEqual(TEST_PAYLOAD)
  })
})
