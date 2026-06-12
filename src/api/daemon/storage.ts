/**
 * Storage API module — typed accessors for daemon storage management endpoints.
 *
 * Storage API 模块 — daemon 存储管理端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /storage/stats` → storage usage statistics
 * - `POST /storage/clear-cache` → clear the blob cache (requires `confirmed: true`)
 *
 * # Transport / 传输 (ADR-008 P7)
 * `getStorageStats` / `clearCache` route through the @hey-api generated SDK
 * (`getStorageStatsSdk` / `clearStorageCacheSdk`) through the daemon client,
 * which drives the daemon session lifecycle: `getStorageStats` uses
 * `daemonClient.callEnveloped` (unwraps down to the payload), `clearCache`
 * stays on `daemonClient.callSdk` (its result is ignored). The public wrapper
 * signatures and the hand-written `StorageStats` domain type below are
 * preserved verbatim for downstream consumers.
 */

import {
  clearStorageCache as clearStorageCacheSdk,
  getStorageStats as getStorageStatsSdk,
} from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

// ── Response types ─────────────────────────────────────────────

/**
 * Storage usage statistics returned by `GET /storage/stats`.
 *
 * `GET /storage/stats` 返回的存储使用统计。
 */
export interface StorageStats {
  totalBytes: number
  databaseBytes: number
  vaultBytes: number
  cacheBytes: number
  logsBytes: number
}

// ── Public API ─────────────────────────────────────────────────

/**
 * Fetch storage usage statistics from the daemon.
 *
 * 从 daemon 获取存储使用统计。
 *
 * @returns StorageStats with entry counts, sizes, and timestamp bounds.
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function getStorageStats(): Promise<StorageStats> {
  // Route through the generated SDK; `callEnveloped` unwraps down to the payload.
  // The generated `StorageStatsDto` is structurally equivalent to the hand-written
  // `StorageStats`, bridged here to keep the public return type stable for
  // downstream consumers.
  const data = await daemonClient.callEnveloped(() => getStorageStatsSdk({ throwOnError: true }))
  return data as unknown as StorageStats
}

/**
 * Request daemon cache clearance.
 *
 * 请求 daemon 清除缓存。
 *
 * The daemon requires `confirmed: true` in the request body. Without it,
 * or with `confirmed: false`, the daemon returns HTTP 400.
 *
 * @param confirmed Must be `true` to trigger the destructive clear operation.
 * @throws {DaemonApiError} On HTTP errors — specifically 400 if `confirmed` is absent/false.
 */
export async function clearCache(confirmed: boolean): Promise<void> {
  // Route through the generated SDK. The endpoint returns an envelope with a
  // `freedBytes` payload, but this wrapper is void, so we do not read `.data`.
  await daemonClient.callSdk(() =>
    clearStorageCacheSdk({ body: { confirmed }, throwOnError: true })
  )
}
