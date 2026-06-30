import { daemonClient } from '@/api/daemon/client'
import {
  deleteClipboardEntry as daemonDeleteEntry,
  restoreClipboardEntry as daemonRestoreEntry,
  toggleFavorite as daemonToggleFavorite,
  clearClipboardHistory as daemonClearHistory,
  getClipboardStats as daemonGetStats,
  getClipboardEntryResource as daemonGetResource,
  getEntryDetail as daemonGetDetail,
} from '@/api/daemon/clipboard'
import { retryLifecycle } from '@/api/lifecycle'
import { revealPath } from '@/api/storage'
import { createLogger } from '@/lib/logger'

const log = createLogger('clipboard-items')

// Detail response type (for fetching full content)
export interface ClipboardEntryDetail {
  id: string
  content: string // Full content
  content_type: string
  size_bytes: number
  is_favorited: boolean
  updated_at: number
  active_time: number
}

export interface ClipboardEntryResource {
  blobId: string | null
  mimeType: string
  sizeBytes: number
  url: string | null
  /** Base64-encoded inline data (present when content is stored inline, not in blob) */
  inlineData: string | null
}

/**
 * 排序选项枚举
 */
export enum OrderBy {
  CreatedAtAsc = 'created_at_asc',
  CreatedAtDesc = 'created_at_desc',
  UpdatedAtAsc = 'updated_at_asc',
  UpdatedAtDesc = 'updated_at_desc',
  ContentTypeAsc = 'content_type_asc',
  ContentTypeDesc = 'content_type_desc',
  IsFavoritedAsc = 'is_favorited_asc',
  IsFavoritedDesc = 'is_favorited_desc',
  ActiveTimeAsc = 'active_time_asc',
  ActiveTimeDesc = 'active_time_desc',
}

/**
 * 过滤选项枚举
 */
export enum Filter {
  All = 'all',
  Favorited = 'favorited',
  Text = 'text',
  Image = 'image',
  RichText = 'richtext',
  Link = 'link',
  Code = 'code',
  File = 'file',
}

/**
 * Map a content-type {@link Filter} to the backend search `contentTypes` param.
 *
 * Single source of truth shared by every search entry point (History page,
 * quick panel) so the type-narrowing rules can't drift. Returns `undefined` for
 * `All`/`Favorited`/`Link`/`Code`/`Image` (those are not physical content
 * types — `link`/`code`/`favorited`/`image` are tags, see
 * {@link filterToTags}).
 *
 * `Image` is a tag, not a content type: a copied image *file* is physically a
 * `file`, and a pure bitmap is physically `image`, but both carry the `image`
 * tag — so filtering by the tag surfaces every image while the `file` filter
 * still finds image files.
 */
export function filterToContentTypes(filter: Filter): string | undefined {
  if (
    filter === Filter.All ||
    filter === Filter.Favorited ||
    filter === Filter.Link ||
    filter === Filter.Code ||
    filter === Filter.Image
  ) {
    return undefined
  }
  if (filter === Filter.RichText) return 'html'
  return filter
}

/**
 * Map a {@link Filter} to the backend search `tags` param, or `undefined` when
 * the filter is not tag-based. `link`/`code`/`favorited`/`image` are derived or
 * user-state tags filtered via the `tags` query parameter (not `contentTypes`).
 */
export function filterToTags(filter: Filter): string | undefined {
  if (filter === Filter.Link) return 'link'
  if (filter === Filter.Code) return 'code'
  if (filter === Filter.Favorited) return 'favorited'
  if (filter === Filter.Image) return 'image'
  return undefined
}

export interface ClipboardStats {
  total_items: number
  total_size: number
}

/**
 * 获取剪贴板统计信息
 */
export async function getClipboardStats(): Promise<ClipboardStats> {
  try {
    const stats = await daemonGetStats()
    return { total_items: stats.totalItems, total_size: stats.totalSize }
  } catch (error) {
    log.error({ err: error }, '获取剪贴板统计信息失败')
    throw error
  }
}

/**
 * Get clipboard entry detail (full content)
 */
export async function getClipboardEntryDetail(id: string): Promise<ClipboardEntryDetail> {
  try {
    const detail = await daemonGetDetail(id)
    if (!detail) throw new Error('Entry detail not found')
    return {
      id: detail.id,
      content: detail.content,
      content_type: detail.mimeType ?? 'text/plain',
      size_bytes: detail.sizeBytes,
      is_favorited: false,
      updated_at: detail.activeTimeMs,
      active_time: detail.activeTimeMs,
    }
  } catch (error) {
    log.error({ err: error }, 'Failed to get clipboard entry detail')
    throw error
  }
}

/**
 * Get clipboard entry resource metadata
 */
export async function getClipboardEntryResource(id: string): Promise<ClipboardEntryResource> {
  try {
    const resource = await daemonGetResource(id)
    if (!resource) throw new Error('Entry resource not found')
    return resource
  } catch (error) {
    log.error({ err: error }, 'Failed to get clipboard entry resource')
    throw error
  }
}

/**
 * Fetch clipboard entry text content via resource URL or inline data
 */
export async function fetchClipboardResourceText(
  resource: ClipboardEntryResource
): Promise<string> {
  try {
    // Use inline data when available (small content stored directly). Check for
    // null explicitly so an empty-string payload ('') decodes to '' instead of
    // falling through to the "neither inlineData nor url" error.
    if (resource.inlineData !== null) {
      const bytes = Uint8Array.from(atob(resource.inlineData), c => c.charCodeAt(0))
      return new TextDecoder('utf-8').decode(bytes)
    }

    // Fall back to URL fetch for blob-backed content. Route through the daemon
    // client so the session token is refreshed (pre-emptive + 401 retry) and
    // never baked into a URL — see `fetchBlob`.
    if (!resource.url) {
      throw new Error('Resource has neither inlineData nor url')
    }
    const blob = await daemonClient.fetchBlob(resource.url)
    return blob.text()
  } catch (error) {
    log.error({ err: error }, 'Failed to fetch clipboard resource text')
    throw error
  }
}

/**
 * Get a token-free image descriptor for a clipboard resource: a `data:` URL for
 * inline content, or the daemon blob path for blob-backed content (which the
 * caller resolves to a `blob:` object URL via {@link useBlobImageObjectUrl} so
 * no short-lived session token ends up in `<img src>`).
 */
export function getResourceImageUrl(resource: ClipboardEntryResource): string | null {
  if (resource.url) {
    return resource.url
  }
  if (resource.inlineData) {
    return `data:${resource.mimeType};base64,${resource.inlineData}`
  }
  return null
}

/**
 * 删除剪贴板条目
 */
export async function deleteClipboardItem(id: string): Promise<boolean> {
  try {
    await daemonDeleteEntry(id)
    return true
  } catch (error) {
    log.error({ err: error }, '删除剪贴板条目失败')
    throw error
  }
}

/**
 * 清空所有剪贴板历史记录
 */
export async function clearClipboardItems(): Promise<number> {
  try {
    const result = await daemonClearHistory()
    return result.deletedCount
  } catch (error) {
    log.error({ err: error }, '清空剪贴板历史记录失败')
    throw error
  }
}

/** Retry daemon lifecycle readiness and deferred clipboard services. */
export async function syncClipboardItems(): Promise<boolean> {
  try {
    await retryLifecycle()
    return true
  } catch (error) {
    log.error({ err: error }, '同步剪贴板内容失败')
    throw error
  }
}

/**
 * 复制剪贴板内容（恢复到系统剪贴板）
 */
export async function copyClipboardItem(id: string): Promise<boolean> {
  try {
    await daemonRestoreEntry(id)
    return true
  } catch (error) {
    log.error({ err: error }, '复制剪贴板记录失败')
    throw error
  }
}

/**
 * 收藏剪贴板条目
 */
export async function favoriteClipboardItem(id: string): Promise<boolean> {
  try {
    await daemonToggleFavorite(id, true)
    return true
  } catch (error) {
    log.error({ err: error }, '收藏剪贴板条目失败')
    throw error
  }
}

/**
 * 取消收藏剪贴板条目
 */
export async function unfavoriteClipboardItem(id: string): Promise<boolean> {
  try {
    await daemonToggleFavorite(id, false)
    return true
  } catch (error) {
    log.error({ err: error }, '取消收藏剪贴板条目失败')
    throw error
  }
}

/**
 * Copy a file entry to the system clipboard via the daemon restore endpoint.
 *
 * Routes through the typed `restoreClipboardEntry` wrapper, which now reads the
 * enveloped `{ data, ts }` restore response (ADR-008 §0.1) and discards the
 * body. The success body is irrelevant here; the 410 `PAYLOAD_UNAVAILABLE`
 * error (whose `entry_id`/`rep_id`/`state` context lives in
 * `ApiErrorResponse.details` per §0.3) still propagates as a `DaemonApiError`
 * so callers can render the "content unavailable" UX.
 */
export async function copyFileToClipboard(entryId: string): Promise<void> {
  await daemonRestoreEntry(entryId)
}

/**
 * Reveal a received file's local copy in the system file manager (opens the
 * containing folder with the item selected).
 *
 * Received files materialize under the app cache dir
 * (`<cache>/iroh-blobs/<entryId>/<filename>`). The daemon projection carries
 * those `file://` URIs in `preview`, which `projectClipboardEntry` decodes into
 * `ClipboardFileItem.file_paths`; callers resolve a concrete native path (see
 * `firstRevealableFilePath`) and pass it here. Delegates to the native
 * `reveal_path` command, which validates existence (404s when the file is gone)
 * and is already used by the log/config-export flows.
 */
export async function openFileLocation(filePath: string): Promise<void> {
  await revealPath(filePath)
}
