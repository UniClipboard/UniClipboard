import type {
  ClipboardCodeItem,
  ClipboardEntry,
  ClipboardEntryContent,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
} from '@/lib/clipboard-entry'

/**
 * Extract a human-readable filename from a file URI (e.g. `file:///path/to/file.txt` → `file.txt`).
 * Handles edge cases: trailing slash (directories), whitespace/CR, non-URL paths, decode failures.
 */
function extractFileNameFromUri(uri: string): string {
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
 * Convert a `file://` URI back to a native filesystem path.
 *
 * The daemon projection carries received files as `file://` URIs (produced by
 * Rust `Url::from_file_path`); the native `reveal_path` command expects a plain
 * filesystem path, so we decode it here. Returns `null` for non-`file://` URIs
 * (e.g. `uniclip-missing://` placeholders) or unparseable input.
 *
 * Windows note: `file:///C:/dir/f.txt` decodes to a pathname of `/C:/dir/f.txt`,
 * so the leading slash before the drive letter is stripped. Received files always
 * land under the local app cache dir, so UNC paths are not expected.
 */
export function fileUriToLocalPath(uri: string): string | null {
  const trimmed = uri.trim()
  if (!trimmed.toLowerCase().startsWith('file://')) return null
  try {
    let pathname = decodeURIComponent(new URL(trimmed).pathname)
    if (/^\/[A-Za-z]:/.test(pathname)) pathname = pathname.slice(1)
    return pathname || null
  } catch {
    return null
  }
}

/**
 * Parse a newline-separated URI list into an array of human-readable filenames.
 */
export function parseFileNamesFromUriList(uriList: string): string[] {
  return uriList.split('\n').flatMap(s => {
    const trimmed = s.trim()
    return trimmed ? [extractFileNameFromUri(trimmed)] : []
  })
}

/**
 * URI scheme used to mark a file blob that didn't complete materializing
 * (typically because the user cancelled the inbound transfer). Receiver still
 * persists the entry so user-facing artifacts (filename / size) survive
 * restart, but the file itself is unavailable for open/copy/drag operations.
 *
 * Format: `uniclip-missing:///<encoded-filename>?size=<bytes>&reason=cancelled`
 */
const UNICLIP_MISSING_SCHEME = 'uniclip-missing:'

function isUniclipMissingUri(uri: string): boolean {
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
  /** Native local path decoded from a `file://` URI, or `null` when absent. */
  path: string | null
  missing: boolean
}> {
  return uriList.split('\n').flatMap(s => {
    const trimmed = s.trim()
    if (!trimmed) return []
    return [
      {
        name: extractFileNameFromUri(trimmed),
        path: fileUriToLocalPath(trimmed),
        missing: isUniclipMissingUri(trimmed),
      },
    ]
  })
}

/**
 * First openable native path in a file entry's content, or `null` when the
 * entry is not a file entry, has no decoded paths, or every file is missing
 * (cancelled transfer). Backs the "open file location" action — revealing any
 * one file opens the containing folder with it selected.
 */
export function firstRevealableFilePath(content: ClipboardEntryContent | null): string | null {
  if (!content || !('file_paths' in content)) return null
  const paths = content.file_paths
  if (!paths) return null
  const missing = content.file_missing
  for (let i = 0; i < paths.length; i++) {
    const path = paths[i]
    if (path && !missing?.[i]) return path
  }
  return null
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

export function getItemPreview(entry: ClipboardEntry): string {
  if (!entry.content) return ''
  switch (entry.type) {
    case 'image': {
      const img = entry.content as ClipboardImageItem
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
      return (entry.content as ClipboardLinkItem).urls[0] ?? ''
    case 'file':
      return (entry.content as ClipboardFileItem).file_names[0] ?? ''
    case 'code':
      return (entry.content as ClipboardCodeItem).code
    case 'text':
      return (entry.content as ClipboardTextItem).display_text
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
