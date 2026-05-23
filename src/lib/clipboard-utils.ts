import type { ClipboardItemResponse } from '@/api/clipboardItems'

export type ItemType = 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'

/**
 * Extract a human-readable filename from a file URI (e.g. `file:///path/to/file.txt` → `file.txt`).
 * Handles edge cases: trailing slash (directories), whitespace/CR, non-URL paths, decode failures.
 */
export function extractFileNameFromUri(uri: string): string {
  const trimmed = uri.trim()
  if (!trimmed) return uri

  // Try standard URL parsing first
  try {
    // Remove trailing slashes to handle directory URIs (e.g. file:///tmp/)
    const pathname = new URL(trimmed).pathname.replace(/\/+$/, '')
    const filename = pathname.split('/').pop()
    if (filename) return decodeURIComponent(filename)
  } catch {
    // Not a valid URL — fall through to path-based extraction
  }

  // Fallback: extract last non-empty segment from raw path string
  const withoutTrailingSlash = trimmed.replace(/\/+$/, '')
  const lastSlash = withoutTrailingSlash.lastIndexOf('/')
  if (lastSlash >= 0 && lastSlash < withoutTrailingSlash.length - 1) {
    const segment = withoutTrailingSlash.substring(lastSlash + 1)
    try {
      return decodeURIComponent(segment)
    } catch {
      return segment
    }
  }

  return trimmed
}

/**
 * Parse a newline-separated URI list into an array of human-readable filenames.
 */
export function parseFileNamesFromUriList(uriList: string): string[] {
  return uriList
    .split('\n')
    .map(s => s.trim())
    .filter(Boolean)
    .map(extractFileNameFromUri)
}

/**
 * URI scheme used to mark a file blob that didn't complete materializing
 * (typically because the user cancelled the inbound transfer). Receiver still
 * persists the entry so user-facing artifacts (filename / size) survive
 * restart, but the file itself is unavailable for open/copy/drag operations.
 *
 * Format: `uniclip-missing:///<encoded-filename>?size=<bytes>&reason=cancelled`
 */
export const UNICLIP_MISSING_SCHEME = 'uniclip-missing:'

export function isUniclipMissingUri(uri: string): boolean {
  const trimmed = uri.trim().toLowerCase()
  return trimmed.startsWith(`${UNICLIP_MISSING_SCHEME}//`)
}

/**
 * Parse a newline-separated URI list into per-file metadata, distinguishing
 * `file://` URIs (real local files) from `uniclip-missing://` placeholders
 * (transfer cancelled before this blob completed).
 *
 * Order of entries matches the URI list line order so callers can zip with
 * `file_sizes` from the daemon projection.
 */
export function parseFileItemsFromUriList(uriList: string): Array<{
  name: string
  missing: boolean
}> {
  return uriList
    .split('\n')
    .map(s => s.trim())
    .filter(Boolean)
    .map(uri => ({
      name: extractFileNameFromUri(uri),
      missing: isUniclipMissingUri(uri),
    }))
}

/**
 * Extract hostname from a URL string. Returns the raw string on failure.
 */
export function extractDomainFromUrl(url: string): string {
  try {
    return new URL(url).hostname
  } catch {
    return url
  }
}

/**
 * Check whether a MIME content type represents an image.
 */
export function isImageContentType(contentType: string): boolean {
  return contentType === 'image' || contentType.startsWith('image/')
}

/**
 * Check whether a MIME content type represents a file (URI list).
 */
export function isFileContentType(contentType: string): boolean {
  return contentType.includes('uri-list')
}

/**
 * 计算 file entry 的 missing 概况。仅在 `item.item.file` 存在时有意义。
 * - `any` = 是否有任意文件被标 missing(决定整条 entry 是否走"取消传输"视觉)
 * - `all` = 是否所有文件都 missing(决定是否禁掉"打开 / 复制 / 拖出")
 *
 * 缺省 `file_missing` 视为全 false(向后兼容历史 entry)。
 */
export function summarizeFileMissing(item: ClipboardItemResponse): {
  any: boolean
  all: boolean
  count: number
} {
  const file = item.item.file
  if (!file) return { any: false, all: false, count: 0 }
  const flags = file.file_missing ?? []
  const total = file.file_names.length
  if (total === 0) return { any: false, all: false, count: 0 }
  const missingCount = flags.filter(Boolean).length
  return {
    any: missingCount > 0,
    all: missingCount === total,
    count: missingCount,
  }
}

export function resolveItemType(item: ClipboardItemResponse): ItemType {
  if (item.item.image) return 'image'
  if (item.item.link) return 'link'
  if (item.item.file) return 'file'
  if (item.item.code) return 'code'
  if (item.item.text) return 'text'
  return 'unknown'
}

export function getItemPreview(item: ClipboardItemResponse): string {
  switch (resolveItemType(item)) {
    case 'image': {
      const img = item.item.image!
      const parts: string[] = ['Image']
      if (img.width > 0 && img.height > 0) parts.push(`${img.width}×${img.height}`)
      if (img.size > 0) {
        if (img.size < 1024) parts.push(`${img.size} B`)
        else if (img.size < 1024 * 1024) parts.push(`${(img.size / 1024).toFixed(1)} KB`)
        else parts.push(`${(img.size / (1024 * 1024)).toFixed(1)} MB`)
      }
      return parts.join(' | ')
    }
    case 'link':
      return item.item.link?.urls[0] ?? ''
    case 'file':
      return item.item.file?.file_names[0] ?? ''
    case 'code':
      return item.item.code?.code ?? ''
    case 'text':
      return item.item.text?.display_text ?? ''
    default:
      return ''
  }
}

export function formatRelativeTime(timestampMs: number): string {
  const diffMs = Date.now() - timestampMs
  const diffMins = Math.round(diffMs / 60000)

  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m`
  if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h`
  return `${Math.floor(diffMins / 1440)}d`
}
