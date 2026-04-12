/**
 * Daemon clipboard API module — typed HTTP client for the daemon clipboard endpoints.
 *
 * Daemon 剪贴板 API 模块 — daemon 剪贴板端点的类型化 HTTP 客户端。
 *
 * # Endpoints / 端点
 * - `GET /clipboard/entries` → paginated list of clipboard entry projections
 * - `GET /clipboard/stats` → aggregate clipboard statistics
 * - `POST /clipboard/entries/clear` → clear all clipboard history
 * - `GET /clipboard/entries/:id` → full entry detail (text content)
 * - `GET /clipboard/entries/:id/resource` → resource metadata (blob/thumbnail)
 * - `POST /clipboard/entries/:id/favorite` → toggle favorite state
 * - `DELETE /clipboard/entries/:id` → delete a single entry
 * - `POST /clipboard/restore/:id` → restore entry to OS clipboard
 *
 * These replace the Tauri `invoke()` calls in `clipboardItems.ts`, enabling the
 * GUI to operate independently of the Tauri layer once the daemon is reachable.
 *
 * 注意：这些端点仅返回预览投影数据（EntryProjectionDto）。
 * 完整条目内容（`ClipboardItemResponse`）需要通过 Tauri 命令获取。
 * 此模块专注于与 daemon HTTP API 的交互。
 */

import { daemonClient } from './client'
import type { RequestOptions } from './client'

// ── HTTP route constants ────────────────────────────────────────

const CLIPBOARD_ENTRIES = '/clipboard/entries'
const CLIPBOARD_STATS = '/clipboard/stats'
const CLIPBOARD_RESTORE = '/clipboard/restore'

// ── Response types matching Rust DTOs ──────────────────────────

/**
 * Single clipboard entry projection from the daemon.
 * Matches `EntryProjectionDto` on the Rust side.
 *
 * Rust 端的 `EntryProjectionDto` 对应。
 */
export interface ClipboardEntryDto {
  id: string
  preview: string
  hasDetail: boolean
  sizeBytes: number
  capturedAt: number
  contentType: string
  thumbnailUrl: string | null
  isEncrypted: boolean
  isFavorited: boolean
  updatedAt: number
  activeTime: number
  /** Aggregate file transfer status for file entries. */
  fileTransferStatus: string | null
  /** Failure reason when fileTransferStatus is "failed". */
  fileTransferReason: string | null
  /** Parsed link URLs for link-type entries. */
  linkUrls: string[] | null
  /** Extracted domains for link entries. */
  linkDomains: string[] | null
  /** Per-file sizes in bytes for file (uri-list) entries. */
  fileSizes: number[] | null
  /** Original image width in pixels (only for image entries). */
  imageWidth?: number | null
  /** Original image height in pixels (only for image entries). */
  imageHeight?: number | null
}

/**
 * GET /clipboard/entries response — discriminated union for readiness.
 *
 * `ClipboardEntriesResponse` 的 TypeScript 版本。
 */
export interface ClipboardEntriesResponse {
  status: 'ready' | 'not_ready'
  entries?: ClipboardEntryDto[]
}

// ── API response wrappers (matching Rust { data, ts } envelope) ──

/** GET /clipboard/entries API envelope. */
interface ListEntriesApiResponse {
  data: ClipboardEntryDto[]
  ts: number
}

/** GET /clipboard/entries/:id API envelope. */
interface GetEntryDetailApiResponse {
  data: EntryDetail
  ts: number
}

/** GET /clipboard/stats API envelope. */
interface GetStatsApiResponse {
  data: ClipboardStats
  ts: number
}

/** GET /clipboard/entries/:id/resource API envelope. */
interface GetResourceApiResponse {
  data: ClipboardEntryResource
  ts: number
}

/** POST /clipboard/entries/clear API envelope. */
interface ClearHistoryApiResponse {
  data: ClearHistoryResult
  ts: number
}

/**
 * Aggregate clipboard statistics.
 * Matches `ClipboardStats` on the Rust side.
 *
 * Rust 端的 `ClipboardStats` 对应。
 */
export interface ClipboardStats {
  totalItems: number
  totalSize: number
}

/**
 * Restore clipboard entry result.
 * The daemon returns 200 OK on success.
 *
 * 恢复剪贴板条目结果。daemon 成功时返回 200 OK。
 */
export interface RestoreResult {
  success: boolean
}

/**
 * Result of clearing all clipboard history.
 * Matches `ClearHistoryResult` on the Rust side.
 *
 * Rust 端的 `ClearHistoryResult` 对应。
 */
export interface ClearHistoryResult {
  deletedCount: number
  failedEntries: [string, string][]
}

/**
 * Full entry detail (text content) from the daemon.
 * Matches `EntryDetailResult` on the Rust side.
 *
 * Rust 端的 `EntryDetailResult` 对应。
 */
export interface EntryDetail {
  id: string
  content: string
  sizeBytes: number
  createdAtMs: number
  activeTimeMs: number
  mimeType: string | null
}

// ── API functions ───────────────────────────────────────────────

/**
 * Fetch paginated clipboard entry projections from the daemon.
 *
 * 从 daemon 获取分页的剪贴板条目投影。
 *
 * @param limit Maximum number of entries to return (default: 50).
 * @param offset Number of entries to skip for pagination (default: 0).
 * @returns `ClipboardEntriesResponse` — use `response.status` to check readiness.
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function getClipboardEntries(
  limit: number = 50,
  offset: number = 0
): Promise<ClipboardEntriesResponse> {
  const params = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  const res = await daemonClient.request<ListEntriesApiResponse>(`${CLIPBOARD_ENTRIES}?${params}`)
  return { status: 'ready', entries: res.data }
}

/**
 * Fetch a single clipboard entry projection by ID.
 *
 * 通过 ID 获取单个剪贴板条目投影。
 *
 * @param id Entry ID.
 * @returns The entry projection, or null if not found or not ready.
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function getClipboardEntry(id: string): Promise<ClipboardEntryDto | null> {
  const params = new URLSearchParams({ id })
  const response = await daemonClient.request<ListEntriesApiResponse>(
    `${CLIPBOARD_ENTRIES}?${params}`
  )

  return response.data?.[0] ?? null
}

/**
 * Delete a clipboard entry by ID.
 *
 * 通过 ID 删除剪贴板条目。
 *
 * @param id Entry ID to delete.
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function deleteClipboardEntry(id: string): Promise<void> {
  const options: RequestOptions = {
    method: 'DELETE',
  }
  await daemonClient.request<void>(`${CLIPBOARD_ENTRIES}/${id}`, options)
}

/**
 * Restore a clipboard entry to the OS clipboard via the daemon.
 * The daemon owns origin tracking and outbound sync.
 *
 * 通过 daemon 将剪贴板条目恢复到系统剪贴板。
 * daemon 负责来源追踪和出站同步。
 *
 * @param id Entry ID to restore.
 * @throws {DaemonApiError} On HTTP errors, session failures, or entry not found.
 */
export async function restoreClipboardEntry(id: string): Promise<RestoreResult> {
  const options: RequestOptions = {
    method: 'POST',
  }
  await daemonClient.request<void>(`${CLIPBOARD_RESTORE}/${id}`, options)
  return { success: true }
}

/**
 * Toggle favorite state for a clipboard entry.
 * Uses POST as defined by the daemon route contract.
 *
 * 切换剪贴板条目的收藏状态。
 * 使用 daemon 路由契约定义的 POST 方法。
 *
 * @param id Entry ID.
 * @param favorited New favorite state.
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function toggleFavorite(id: string, favorited: boolean): Promise<void> {
  const options: RequestOptions = {
    method: 'POST',
    body: { isFavorited: favorited },
  }
  await daemonClient.request<void>(`${CLIPBOARD_ENTRIES}/${id}/favorite`, options)
}

/**
 * Clear all clipboard history via the daemon bulk delete endpoint.
 * Returns the number of entries deleted and any failures.
 *
 * 通过 daemon 批量删除端点清除所有剪贴板历史。
 *
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function clearClipboardHistory(): Promise<ClearHistoryResult> {
  const options: RequestOptions = {
    method: 'POST',
  }
  const res = await daemonClient.request<ClearHistoryApiResponse>(
    `${CLIPBOARD_ENTRIES}/clear`,
    options
  )
  return res.data
}

/**
 * Fetch full entry detail (text content) for a given entry ID.
 * Returns 404 for non-text content or missing entries.
 *
 * 获取给定条目的完整文本内容详情。
 * 非文本内容或缺失条目返回 404。
 *
 * @param id Entry ID.
 * @returns Entry detail or null if not found.
 * @throws {DaemonApiError} On HTTP errors or session failures (excluding not-found).
 */
export async function getEntryDetail(id: string): Promise<EntryDetail | null> {
  try {
    const res = await daemonClient.request<GetEntryDetailApiResponse>(`${CLIPBOARD_ENTRIES}/${id}`)
    return res.data
  } catch (error) {
    if (
      error instanceof Error &&
      'code' in error &&
      (error as { code: string }).code === 'NOT_FOUND'
    ) {
      return null
    }
    throw error
  }
}

/**
 * Fetch aggregate clipboard statistics.
 *
 * 获取剪贴板统计信息。
 *
 * @returns `ClipboardStats` with total item count and total size.
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function getClipboardStats(): Promise<ClipboardStats> {
  const res = await daemonClient.request<GetStatsApiResponse>(CLIPBOARD_STATS)
  return res.data
}

/**
 * Get clipboard entry resource metadata (blob URL, inline data, MIME type).
 *
 * 获取剪贴板条目资源元信息（blob URL、内联数据、MIME 类型）。
 *
 * Note: This returns the resource metadata only — use `fetchClipboardResourceText()`
 * from `clipboardItems.ts` to decode inline content or fetch blob data.
 *
 * @param id Entry ID.
 * @returns Resource metadata or null if not found.
 * @throws {DaemonApiError} On HTTP errors or session failures.
 */
export async function getClipboardEntryResource(
  id: string
): Promise<ClipboardEntryResource | null> {
  try {
    const res = await daemonClient.request<GetResourceApiResponse>(
      `${CLIPBOARD_ENTRIES}/${id}/resource`
    )
    return res.data
  } catch (error) {
    // Return null for not-found rather than throwing
    if (
      error instanceof Error &&
      'code' in error &&
      (error as { code: string }).code === 'NOT_FOUND'
    ) {
      return null
    }
    throw error
  }
}

/**
 * Alias for getEntryDetail — maintains compatibility with existing code
 * that uses the Tauri-style naming convention.
 */
export { getEntryDetail as getClipboardEntryDetail }

/**
 * Clipboard entry resource metadata.
 * Matches `ClipboardEntryResource` on the Rust side.
 *
 * Rust 端的 `ClipboardEntryResource` 对应。
 */
export interface ClipboardEntryResource {
  blobId: string | null
  mimeType: string
  sizeBytes: number
  url: string | null
  inlineData: string | null
}
