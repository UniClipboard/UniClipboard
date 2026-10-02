/**
 * Presence API — typed accessors for daemon presence endpoints.
 *
 * # Endpoints / 端点
 * - `POST /presence/refresh` → 主动 probe 一轮所有已配对 peer 的连接性
 *
 * 用途：缩短"对端断网"场景下的离线检测时延。后端 watchdog 依赖 QUIC
 * `max_idle_timeout = 60s` 才能感知断连；前端在 DevicesPage 可见时定期
 * probe，可在 ~probe 间隔 + 拨号失败时间内反馈离线。probe 过程中真实的
 * Online/Offline 变化会通过既有 `peers.changed` WebSocket 链路推到前端。
 */

import { refreshPresence as refreshPresenceSdk } from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

export interface PresenceRefreshResult {
  total: number
  online: number
  offline: number
  errors: number
}

/**
 * 触发一轮 ensure_reachable_all 探测。
 *
 * 后端会对所有已配对 peer 并发拨号；离线 peer 立即被标记 Offline，进而
 * 推送 `peers.changed`，前端再走 fetchSpaceMembers 重拉刷新 UI。
 *
 * # Transport / 传输 (ADR-008 P7)
 * Routes through the @hey-api generated SDK (`refreshPresence`) via
 * `daemonClient.callEnveloped`, which drives the daemon session lifecycle and
 * unwraps down to the probe-counter payload. The generated
 * `PresenceRefreshResponse` is structurally equivalent to the hand-written
 * `PresenceRefreshResult`, bridged at the boundary to keep the public return
 * type stable for downstream consumers.
 */
export async function refreshPresence(): Promise<PresenceRefreshResult> {
  const data = await daemonClient.callEnveloped(() => refreshPresenceSdk({ throwOnError: true }))
  return data as unknown as PresenceRefreshResult
}
