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
    case 'image':
      return 'Image'
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
