/**
 * Storage API module — typed accessors for daemon storage management endpoints.
 *
 * Storage API 模块 — daemon 存储管理端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /storage/stats` → storage usage statistics
 * - `POST /storage/clear-cache` → clear the blob cache (requires `confirmed: true`)
 */

import { daemonClient } from './daemon/client'

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

interface StorageStatsEnvelope {
  data: StorageStats
  ts: number
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
  const res = await daemonClient.request<StorageStatsEnvelope>('/storage/stats')
  return res.data
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
  await daemonClient.request<void>('/storage/clear-cache', {
    method: 'POST',
    body: { confirmed },
  })
}

/**
 * Open the platform data directory in the system file explorer.
 * This is the only Tauri invoke remaining in the storage module — it requires
 * native OS integration that the daemon cannot provide.
 *
 * 打开平台数据目录的系统文件浏览器。
 * 这是存储模块中唯一保留的 Tauri invoke — 它需要 daemon 无法提供的原生 OS 集成。
 */
export async function openDataDirectory(): Promise<void> {
  const { commands } = await import('@/lib/ipc')
  await commands.openDataDirectory()
}

// Re-export clipboard history clearance from daemon clipboard API.
// This is used by StorageSection for the "clear all history" action.
export { clearClipboardHistory as clearAllClipboardHistory } from './daemon/clipboard'
