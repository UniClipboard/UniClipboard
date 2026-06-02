/**
 * Integration tests for the daemon lifecycle API module.
 *
 * Uses vi.spyOn to track daemonClient.request calls while preserving
 * the real function logic.
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

describe('Lifecycle API', () => {
  let requestSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    requestSpy = vi.spyOn(daemonClient, 'request')
    requestSpy.mockResolvedValue(undefined)
  })

  afterEach(() => {
    requestSpy.mockRestore()
  })

  describe('signalLifecycleReady', () => {
    it('posts lifecycle ready to the daemon via POST /lifecycle/ready', async () => {
      await signalLifecycleReady()

      expect(requestSpy).toHaveBeenCalledTimes(1)
      expect(requestSpy).toHaveBeenCalledWith('/lifecycle/ready', { method: 'POST' })
    })
  })

  describe('getLifecycleStatus', () => {
    it('calls GET /lifecycle/status and returns parsed dto', async () => {
      // ADR-008: GET /lifecycle/status returns `{ data: { state }, ts }`.
      requestSpy.mockResolvedValueOnce({ data: { state: 'Ready' }, ts: 0 })

      const result = await getLifecycleStatus()

      expect(requestSpy).toHaveBeenCalledTimes(1)
      expect(requestSpy).toHaveBeenCalledWith('/lifecycle/status')
      expect(result.state).toBe('Ready')
    })

    it.each(['Idle', 'Pending', 'Ready', 'WatcherFailed', 'NetworkFailed'])(
      'handles state: %s',
      async state => {
        requestSpy.mockResolvedValueOnce({ data: { state }, ts: 0 })

        const result = await getLifecycleStatus()

        expect(result.state).toBe(state)
      }
    )

    it('re-throws error on HTTP failure', async () => {
      requestSpy.mockRejectedValueOnce(new Error('internal error'))

      await expect(getLifecycleStatus()).rejects.toThrow('internal error')
    })
  })

  describe('retryLifecycle', () => {
    it('calls POST /lifecycle/retry and returns void', async () => {
      await retryLifecycle()

      expect(requestSpy).toHaveBeenCalledTimes(1)
      expect(requestSpy).toHaveBeenCalledWith('/lifecycle/retry', { method: 'POST' })
    })

    it('re-throws error on HTTP failure', async () => {
      requestSpy.mockRejectedValueOnce(new Error('retry failed'))

      await expect(retryLifecycle()).rejects.toThrow('retry failed')
    })
  })
})
