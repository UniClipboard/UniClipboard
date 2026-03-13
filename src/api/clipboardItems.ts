import { resolveUcUrl } from '@/lib/protocol'
import { invokeWithTrace } from '@/lib/tauri-command'

// Backend projection type
interface ClipboardEntryProjection {
  id: string
  preview: string // Preview content (may be truncated)
  has_detail: boolean // Whether full detail is available
  size_bytes: number
  captured_at: number
  content_type: string
  is_encrypted: boolean
  is_favorited: boolean
  updated_at: number
  active_time: number
  thumbnail_url?: string | null
  /** Parsed link URLs (built from full representation data, not preview) */
  link_urls?: string[] | null
  /** Extracted domains for link entries */
  link_domains?: string[] | null
}

type ClipboardEntriesResponse =
  | { status: 'ready'; entries: ClipboardEntryProjection[] }
  | { status: 'not_ready' }

export type ClipboardItemsResult =
  | { status: 'ready'; items: ClipboardItemResponse[] }
  | { status: 'not_ready' }

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
  blob_id: string | null
  mime_type: string
  size_bytes: number
  url: string | null
  /** Base64-encoded inline data (present when content is stored inline, not in blob) */
  inline_data: string | null
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
  Link = 'link',
  Code = 'code',
  File = 'file',
}

export interface ClipboardTextItem {
  display_text: string // Changed: now always shows preview
  has_detail: boolean // NEW: replaced is_truncated, indicates if full content is available
  size: number
}

export interface ClipboardImageItem {
  thumbnail?: string | null
  size: number
  width: number
  height: number
}

export interface ClipboardFileItem {
  file_names: string[]
  file_sizes: number[]
}

export interface ClipboardLinkItem {
  urls: string[]
  domains: string[]
}

export interface ClipboardCodeItem {
  code: string
}

export interface ClipboardItem {
  text?: ClipboardTextItem | null
  image?: ClipboardImageItem | null
  file?: ClipboardFileItem | null
  link?: ClipboardLinkItem | null
  code?: ClipboardCodeItem | null
  unknown?: null
}

export interface ClipboardItemResponse {
  id: string
  is_downloaded: boolean
  is_favorited: boolean
  created_at: number
  updated_at: number
  active_time: number
  item: ClipboardItem
}

export interface ClipboardStats {
  total_items: number
  total_size: number
}

/**
 * Extract hostname from a URL string. Returns the raw string on failure.
 */
function extractDomainFromUrl(url: string): string {
  try {
    return new URL(url).hostname
  } catch {
    return url
  }
}

/**
 * Transform a backend ClipboardEntryProjection to frontend ClipboardItemResponse.
 * Shared by getClipboardItems and getClipboardEntry to avoid duplication.
 */
function transformProjectionToResponse(entry: ClipboardEntryProjection): ClipboardItemResponse {
  const isImage = isImageType(entry.content_type)

  // Use pre-parsed link data from backend (built from full representation, not preview)
  const hasLinkData = !isImage && entry.link_urls && entry.link_urls.length > 0
  let linkItem: ClipboardLinkItem | null = null
  if (hasLinkData) {
    linkItem = {
      urls: entry.link_urls!,
      domains: entry.link_domains ?? entry.link_urls!.map(extractDomainFromUrl),
    }
  }

  const item: ClipboardItem = {
    image: isImage
      ? {
          thumbnail: entry.thumbnail_url ?? null,
          size: entry.size_bytes,
          width: 0,
          height: 0,
        }
      : null,
    text:
      !isImage && !hasLinkData
        ? {
            display_text: entry.preview,
            has_detail: entry.has_detail,
            size: entry.size_bytes,
          }
        : null,
    file: null as unknown as ClipboardFileItem,
    link: linkItem as unknown as ClipboardLinkItem,
    code: null as unknown as ClipboardCodeItem,
    unknown: null,
  }

  return {
    id: entry.id,
    is_downloaded: true,
    is_favorited: entry.is_favorited,
    created_at: entry.captured_at,
    updated_at: entry.updated_at,
    active_time: entry.active_time,
    item,
  }
}

/**
 * 获取剪贴板统计信息
 * @returns Promise，返回剪贴板统计信息
 */
export async function getClipboardStats(): Promise<ClipboardStats> {
  try {
    return await invokeWithTrace('get_clipboard_stats')
  } catch (error) {
    console.error('获取剪贴板统计信息失败:', error)
    throw error
  }
}

/**
 * 获取剪贴板历史记录
 * @param orderBy 排序方式（暂未实现）
 * @param limit 限制返回的条目数
 * @param offset 偏移量，用于分页（暂未实现）
 * @param filter 过滤选项（暂未实现）
 * @returns Promise，返回剪贴板条目数组
 */
export async function getClipboardItems(
  _orderBy?: OrderBy,
  limit?: number,
  offset?: number,
  _filter?: Filter
): Promise<ClipboardItemsResult> {
  try {
    // Note: orderBy and filter are not yet implemented in the backend command
    // Map Filter enum to backend format if needed (for future use)
    // const mappedFilter = filter === Filter.All ? undefined : filter

    // Use new command name: get_clipboard_entries
    const response = await invokeWithTrace<ClipboardEntriesResponse>('get_clipboard_entries', {
      limit: limit ?? 50,
      offset: offset ?? 0,
    })

    if (response.status === 'not_ready') {
      return { status: 'not_ready' }
    }

    // Transform backend projection to frontend response format
    const items = response.entries.map(transformProjectionToResponse)

    return { status: 'ready', items }
  } catch (error) {
    console.error('获取剪贴板历史记录失败:', error)
    throw error
  }
}

/**
 * Fetch a single clipboard entry by ID using the new get_clipboard_entry command.
 * Returns the transformed ClipboardItemResponse, or null if not ready / not found.
 */
export async function getClipboardEntry(entryId: string): Promise<ClipboardItemResponse | null> {
  try {
    const response = await invokeWithTrace<ClipboardEntriesResponse>('get_clipboard_entry', {
      entryId,
    })

    if (response.status === 'not_ready' || response.entries.length === 0) {
      return null
    }

    return transformProjectionToResponse(response.entries[0])
  } catch (error) {
    console.error('Failed to get clipboard entry:', error)
    return null
  }
}

/**
 * 获取单个剪贴板条目
 * @param id 剪贴板条目ID
 * @param fullContent 是否获取完整内容，不进行截断
 * @returns Promise，返回剪贴板条目，若不存在则返回null
 */
export async function getClipboardItem(
  id: string,
  fullContent: boolean = false
): Promise<ClipboardItemResponse | null> {
  try {
    return await invokeWithTrace('get_clipboard_item', { id, fullContent })
  } catch (error) {
    console.error('获取剪贴板条目失败:', error)
    throw error
  }
}

/**
 * Get clipboard entry detail (full content)
 * 获取剪切板条目详情（完整内容）
 * @param id Entry ID
 * @returns Promise with full entry detail
 */
export async function getClipboardEntryDetail(id: string): Promise<ClipboardEntryDetail> {
  try {
    return await invokeWithTrace('get_clipboard_entry_detail', { entryId: id })
  } catch (error) {
    console.error('Failed to get clipboard entry detail:', error)
    throw error
  }
}

/**
 * Get clipboard entry resource metadata
 * 获取剪切板条目资源元信息
 * @param id Entry ID
 * @returns Promise with resource metadata
 */
export async function getClipboardEntryResource(id: string): Promise<ClipboardEntryResource> {
  try {
    return await invokeWithTrace('get_clipboard_entry_resource', { entryId: id })
  } catch (error) {
    console.error('Failed to get clipboard entry resource:', error)
    throw error
  }
}

/**
 * Fetch clipboard entry text content via resource URL or inline data
 * 通过资源 URL 或内联数据拉取并解码剪贴板文本内容
 */
export async function fetchClipboardResourceText(
  resource: ClipboardEntryResource
): Promise<string> {
  try {
    // Use inline data when available (small content stored directly)
    if (resource.inline_data) {
      const bytes = Uint8Array.from(atob(resource.inline_data), c => c.charCodeAt(0))
      return new TextDecoder('utf-8').decode(bytes)
    }

    // Fall back to URL fetch for blob-backed content
    if (!resource.url) {
      throw new Error('Resource has neither inline_data nor url')
    }
    const resolvedUrl = resolveUcUrl(resource.url)
    const response = await fetch(resolvedUrl)
    if (!response.ok) {
      throw new Error(`Failed to fetch clipboard resource: ${response.status}`)
    }
    const buffer = await response.arrayBuffer()
    return new TextDecoder('utf-8').decode(buffer)
  } catch (error) {
    console.error('Failed to fetch clipboard resource text:', error)
    throw error
  }
}

/**
 * Get a displayable image URL from a clipboard resource.
 * Uses blob URL when available, falls back to data URL from inline data.
 * 从剪贴板资源获取可显示的图片 URL。
 */
export function getResourceImageUrl(resource: ClipboardEntryResource): string | null {
  if (resource.url) {
    return resource.url
  }
  if (resource.inline_data) {
    return `data:${resource.mime_type};base64,${resource.inline_data}`
  }
  return null
}

/**
 * 删除剪贴板条目
 * @param id 剪贴板条目ID
 * @returns Promise，成功返回true
 */
export async function deleteClipboardItem(id: string): Promise<boolean> {
  try {
    return await invokeWithTrace('delete_clipboard_entry', { entryId: id })
  } catch (error) {
    console.error('删除剪贴板条目失败:', error)
    throw error
  }
}

/**
 * 清空所有剪贴板历史记录
 * @returns Promise，成功返回删除的条目数
 */
export async function clearClipboardItems(): Promise<number> {
  try {
    return await invokeWithTrace('clear_clipboard_items')
  } catch (error) {
    console.error('清空剪贴板历史记录失败:', error)
    throw error
  }
}

/**
 * 同步剪贴板内容
 * @returns Promise，成功返回true
 */
export async function syncClipboardItems(): Promise<boolean> {
  try {
    return await invokeWithTrace('sync_clipboard_items')
  } catch (error) {
    console.error('同步剪贴板内容失败:', error)
    throw error
  }
}

/**
 * 复制剪贴板内容
 * @param id 剪贴板条目ID
 * @returns Promise，成功返回true
 */
export async function copyClipboardItem(id: string): Promise<boolean> {
  try {
    return await invokeWithTrace('restore_clipboard_entry', { entryId: id })
  } catch (error) {
    console.error('复制剪贴板记录失败:', error)
    throw error
  }
}

/**
 * 根据内容类型获取符合前端显示的类型
 * @param contentType 内容类型字符串
 * @returns 适合UI显示的类型
 */
export function getDisplayType(
  item: ClipboardItem
): 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown' {
  if (item.text) {
    return 'text'
  } else if (item.image) {
    return 'image'
  } else if (item.file) {
    return 'file'
  } else if (item.link) {
    return 'link'
  } else if (item.code) {
    return 'code'
  } else {
    return 'unknown'
  }
}

/**
 * 判断是否为图片类型
 * @param contentType 内容类型
 * @returns 是否为图片
 */
export function isImageType(contentType: string): boolean {
  return contentType === 'image' || contentType.startsWith('image/')
}

/**
 * 判断是否为文本类型
 * @param contentType 内容类型
 * @returns 是否为文本
 */
export function isTextType(contentType: string): boolean {
  return contentType === 'text' || contentType.startsWith('text/')
}

/**
 * 收藏剪贴板条目
 * @param id 剪贴板条目ID
 * @returns Promise，成功返回true
 */
export async function favoriteClipboardItem(id: string): Promise<boolean> {
  try {
    return await invokeWithTrace('toggle_favorite_clipboard_item', { id, is_favorited: true })
  } catch (error) {
    console.error('收藏剪贴板条目失败:', error)
    throw error
  }
}

/**
 * 取消收藏剪贴板条目
 * @param id 剪贴板条目ID
 * @returns Promise，成功返回true
 */
export async function unfavoriteClipboardItem(id: string): Promise<boolean> {
  try {
    return await invokeWithTrace('toggle_favorite_clipboard_item', { id, is_favorited: false })
  } catch (error) {
    console.error('取消收藏剪贴板条目失败:', error)
    throw error
  }
}
