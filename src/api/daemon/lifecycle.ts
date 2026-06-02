/**
 * Lifecycle API module — typed accessors for daemon lifecycle endpoints.
 *
 * # Endpoints / 端点
 * - `POST /lifecycle/ready` → notify the daemon that the GUI is ready for clipboard capture
 * - `GET /lifecycle/status` → current lifecycle status
 * - `POST /lifecycle/retry` → retry lifecycle initialization
 */

import type { LifecycleStatusDto, LifecycleStatusEnvelope } from '@/api/types'
import { daemonClient } from './client'

interface LifecycleReadyResponse {
  data?: { success: boolean }
  ts?: number
}

/**
 * Notify the daemon that the GUI is ready and deferred services can start.
 */
export async function signalLifecycleReady(): Promise<void> {
  await daemonClient.request<LifecycleReadyResponse>('/lifecycle/ready', {
    method: 'POST',
  })
}

/**
 * Get current lifecycle status from the daemon.
 *
 * 获取 daemon 的当前生命周期状态。
 *
 * @returns LifecycleStatusDto with state: 'Idle' | 'Pending' | 'Ready' | 'WatcherFailed' | 'NetworkFailed'
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function getLifecycleStatus(): Promise<LifecycleStatusDto> {
  // `/lifecycle/status` now returns the canonical `{ data, ts }` envelope
  // (ADR-008 §H). Unwrap `data` so the public return type stays unchanged.
  const res = await daemonClient.request<LifecycleStatusEnvelope>('/lifecycle/status')
  return res.data
}

/**
 * Trigger lifecycle retry via the daemon.
 * The daemon will re-attempt initialization. No body is required; success is 204 No Content.
 *
 * 触发 daemon 重新初始化生命周期。
 *
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function retryLifecycle(): Promise<void> {
  await daemonClient.request<void>('/lifecycle/retry', { method: 'POST' })
}
