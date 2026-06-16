/**
 * Storage API module — typed accessors for daemon storage management endpoints.
 *
 * Storage API 模块 — daemon 存储管理端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /storage/stats` → storage usage statistics
 * - `POST /storage/clear-cache` → clear the blob cache (requires `confirmed: true`)
 *
 * # Note / 注意
 * `getStorageStats`, `clearCache`, and the `StorageStats` type are the single
 * source of truth in `./daemon/storage` (ADR-008 P7). This module re-exports them
 * so existing consumers importing from `@/api/storage` keep working unchanged,
 * while adding the OS-integration helpers (`openDataDirectory` / `openLogsDirectory`)
 * that the daemon cannot provide.
 */

export { getStorageStats, clearCache } from './daemon/storage'
export type { StorageStats } from './daemon/storage'

// ── Public API ─────────────────────────────────────────────────

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

/**
 * Open the platform logs directory in the system file explorer.
 *
 * 打开平台日志目录的系统文件浏览器。
 */
export async function openLogsDirectory(): Promise<void> {
  const { commands } = await import('@/lib/ipc')
  await commands.openLogsDirectory()
}

/**
 * Reveal a file or directory in the system file explorer, opening its
 * containing folder with the item selected. Used to show the user where an
 * exported log archive landed.
 *
 * 在系统文件浏览器中定位文件/目录：打开其所在目录并选中该项。
 * 用于在日志导出后向用户展示归档文件所在位置。
 */
export async function revealPath(path: string): Promise<void> {
  const { commands } = await import('@/lib/ipc')
  await commands.revealPath(path)
}

// Re-export clipboard history clearance from daemon clipboard API.
// This is used by StorageSection for the "clear all history" action.
export { clearClipboardHistory as clearAllClipboardHistory } from './daemon/clipboard'
