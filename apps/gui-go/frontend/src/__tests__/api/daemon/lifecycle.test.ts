/**
 * Integration tests for the daemon lifecycle API module.
 *
 * ADR-008 P7: lifecycle wrappers route through the @hey-api generated SDK via
 * `daemonClient.callSdk`. Tests spy on `callSdk` (replaying the real happy path
 * `const { data } = await call(); return data`) and drive the SDK-fn mocks so
 * the wrapper sees the SDK's `{ data: <envelope> }` shape.
 *
 * Covers:
 * - POST /lifecycle/ready
 * - GET /lifecycle/status
 * - POST /lifecycle/retry
 *
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { signalLifecycleReady, getLifecycleStatus, retryLifecycle } from '@/api/daemon/lifecycle'
import {
  getLifecycleStatus as getLifecycleStatusSdk,
  retryLifecycle as retryLifecycleSdk,
  signalLifecycleReady as signalLifecycleReadySdk,
} from '@/api/generated/sdk.gen'

vi.mock('@/api/generated/sdk.gen', () => ({
  getLifecycleStatus: vi.fn(),
  retryLifecycle: vi.fn(),
  signalLifecycleReady: vi.fn(),
}))

const readySdkMock = signalLifecycleReadySdk as unknown as ReturnType<typeof vi.fn>
const statusSdkMock = getLifecycleStatusSdk as unknown as ReturnType<typeof vi.fn>
const retrySdkMock = retryLifecycleSdk as unknown as ReturnType<typeof vi.fn>

describe('Lifecycle API', () => {
  let callSdkSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    // Replay the real callSdk happy path: unwrap the SDK's outer `{ data }`.
    callSdkSpy = vi
      .spyOn(daemonClient, 'callSdk')
      .mockImplementation((call: () => Promise<{ data: unknown }>) =>
        call().then(r => r.data)
      ) as ReturnType<typeof vi.spyOn>
    readySdkMock.mockReset()
    statusSdkMock.mockReset()
    retrySdkMock.mockReset()
    // 204 endpoints resolve to `{ data: undefined }` by default.
    readySdkMock.mockResolvedValue({ data: undefined })
    retrySdkMock.mockResolvedValue({ data: undefined })
  })

  afterEach(() => {
    callSdkSpy.mockRestore()
  })

  describe('signalLifecycleReady', () => {
    it('posts lifecycle ready to the daemon via the SDK fn', async () => {
      await signalLifecycleReady()

      expect(readySdkMock).toHaveBeenCalledTimes(1)
      expect(readySdkMock).toHaveBeenCalledWith({ throwOnError: true })
    })
  })

  describe('getLifecycleStatus', () => {
    it('calls the status SDK fn and returns parsed dto', async () => {
      // ADR-008: GET /lifecycle/status returns `{ data: { state }, ts }`;
      // the SDK fn resolves to `{ data: <envelope> }`.
      statusSdkMock.mockResolvedValueOnce({ data: { data: { state: 'Ready' }, ts: 0 } })

      const result = await getLifecycleStatus()

      expect(statusSdkMock).toHaveBeenCalledTimes(1)
      expect(statusSdkMock).toHaveBeenCalledWith({ throwOnError: true })
      expect(result.state).toBe('Ready')
    })

    it.each(['Idle', 'Pending', 'Ready', 'WatcherFailed', 'NetworkFailed'])(
      'handles state: %s',
      async state => {
        statusSdkMock.mockResolvedValueOnce({ data: { data: { state }, ts: 0 } })

        const result = await getLifecycleStatus()

        expect(result.state).toBe(state)
      }
    )

    it('re-throws error on HTTP failure', async () => {
      statusSdkMock.mockRejectedValueOnce(new Error('internal error'))

      await expect(getLifecycleStatus()).rejects.toThrow('internal error')
    })
  })

  describe('retryLifecycle', () => {
    it('calls the retry SDK fn and returns void', async () => {
      await retryLifecycle()

      expect(retrySdkMock).toHaveBeenCalledTimes(1)
      expect(retrySdkMock).toHaveBeenCalledWith({ throwOnError: true })
    })

    it('re-throws error on HTTP failure', async () => {
      retrySdkMock.mockRejectedValueOnce(new Error('retry failed'))

      await expect(retryLifecycle()).rejects.toThrow('retry failed')
    })
  })
})
