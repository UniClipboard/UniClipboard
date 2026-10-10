/**
 * Lifecycle API module — typed accessors for daemon lifecycle endpoints.
 *
 * # Endpoints / 端点
 * - `POST /lifecycle/ready` → notify the daemon that the GUI is ready for clipboard capture
 * - `GET /lifecycle/status` → current lifecycle status
 * - `POST /lifecycle/retry` → retry lifecycle initialization
 *
 * # Transport / 传输 (ADR-008 P7)
 * Routes through the @hey-api generated SDK via the daemon client, which
 * drives the daemon session lifecycle (pre-emptive refresh + one-shot 401
 * retry). Value-returning endpoints use `daemonClient.callEnveloped`, which
 * unwraps the `ApiEnvelope` down to the payload; 204 endpoints use
 * `daemonClient.callSdk`. The public wrapper signatures and the hand-written
 * `LifecycleStatusDto` domain type are preserved verbatim for downstream
 * consumers.
 */

import {
  getLifecycleStatus as getLifecycleStatusSdk,
  retryLifecycle as retryLifecycleSdk,
  signalLifecycleReady as signalLifecycleReadySdk,
} from '@/api/generated/sdk.gen'
import type { LifecycleStatusDto } from '@/api/types'
import { daemonClient } from './client'

/**
 * Notify the daemon that the GUI is ready and deferred services can start.
 */
export async function signalLifecycleReady(): Promise<void> {
  // 204 No Content endpoint with no body; do not read `.data`.
  await daemonClient.callSdk(() => signalLifecycleReadySdk({ throwOnError: true }))
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
  // `/lifecycle/status` returns the canonical `{ data, ts }` envelope
  // (ADR-008 §H). `callEnveloped` unwraps down to the payload. The generated
  // `LifecycleStatusResponse` (`{ state: string }`) is bridged to the
  // hand-written `LifecycleStatusDto` (union-typed `state`) so the public
  // return type stays unchanged.
  const data = await daemonClient.callEnveloped(() => getLifecycleStatusSdk({ throwOnError: true }))
  return data as unknown as LifecycleStatusDto
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
  // 204 No Content endpoint with no body; do not read `.data`.
  await daemonClient.callSdk(() => retryLifecycleSdk({ throwOnError: true }))
}
