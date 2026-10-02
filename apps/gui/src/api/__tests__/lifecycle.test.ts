/**
 * Tests for the lifecycle API facade.
 * Verifies that getLifecycleStatus and retryLifecycle delegate to daemon HTTP endpoints.
 *
 * ADR-008 P7: the daemon lifecycle wrappers route through the @hey-api generated
 * SDK via `daemonClient.callSdk`, so this facade test spies on `callSdk` and
 * drives the SDK-fn mocks instead of `daemonClient.request`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import {
  getLifecycleStatus as getLifecycleStatusSdk,
  retryLifecycle as retryLifecycleSdk,
} from '@/api/generated/sdk.gen'
import { getLifecycleStatus, retryLifecycle } from '@/api/lifecycle'
import type { LifecycleStatusDto } from '@/api/types'

vi.mock('@/api/generated/sdk.gen', () => ({
  getLifecycleStatus: vi.fn(),
  retryLifecycle: vi.fn(),
  signalLifecycleReady: vi.fn(),
}))

const statusSdkMock = getLifecycleStatusSdk as unknown as ReturnType<typeof vi.fn>
const retrySdkMock = retryLifecycleSdk as unknown as ReturnType<typeof vi.fn>

describe('lifecycle api facade', () => {
  let callSdkSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    callSdkSpy = vi
      .spyOn(daemonClient, 'callSdk')
      .mockImplementation((call: () => Promise<{ data: unknown }>) =>
        call().then(r => r.data)
      ) as ReturnType<typeof vi.spyOn>
    statusSdkMock.mockReset()
    retrySdkMock.mockReset()
    retrySdkMock.mockResolvedValue({ data: undefined })
  })

  afterEach(() => {
    callSdkSpy.mockRestore()
  })

  it('getLifecycleStatus calls daemon getLifecycleStatus', async () => {
    // ADR-008: GET /lifecycle/status returns ApiEnvelope<LifecycleStatusDto>
    // = `{ data: { state }, ts }`; the SDK fn resolves to `{ data: <envelope> }`.
    const payload: LifecycleStatusDto = { state: 'Ready' }
    statusSdkMock.mockResolvedValueOnce({ data: { data: payload, ts: 0 } })

    const result = await getLifecycleStatus()

    expect(statusSdkMock).toHaveBeenCalledTimes(1)
    expect(statusSdkMock).toHaveBeenCalledWith({ throwOnError: true })
    expect(result).toEqual(payload)
    expect(result.state).toBe('Ready')
  })

  it('getLifecycleStatus returns typed dto with lifecycleState union', async () => {
    statusSdkMock.mockResolvedValueOnce({ data: { data: { state: 'Pending' }, ts: 0 } })

    const result = await getLifecycleStatus()

    expect(result.state).toBe('Pending')
  })

  it('retryLifecycle calls daemon retryLifecycle', async () => {
    await retryLifecycle()

    expect(retrySdkMock).toHaveBeenCalledTimes(1)
    expect(retrySdkMock).toHaveBeenCalledWith({ throwOnError: true })
  })

  it('getLifecycleStatus re-throws daemon errors', async () => {
    statusSdkMock.mockRejectedValueOnce(new Error('daemon unavailable'))

    await expect(getLifecycleStatus()).rejects.toThrow('daemon unavailable')
  })

  it('retryLifecycle re-throws daemon errors', async () => {
    retrySdkMock.mockRejectedValueOnce(new Error('lifecycle retry failed'))

    await expect(retryLifecycle()).rejects.toThrow('lifecycle retry failed')
  })
})
