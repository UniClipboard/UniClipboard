/**
 * Daemon DTO → UI domain projection.
 *
 * The single place where the daemon's `EntryProjectionDto` wire shape is
 * mapped to the `ClipboardEntry` domain model. Every list/event path
 * (`clipboardSlice`, `useClipboardCollection`, `useClipboardEventStream`)
 * goes through this function, so daemon field changes only impact this file.
 */
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import type { SearchResultDto } from '@/api/daemon/search'
import type {
  ClipboardEntry,
  ClipboardEntryContent,
  ClipboardEntryTag,
  ClipboardEntryType,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import {
  extractDomainFromUrl,
  isFileContentType,
  isImageContentType,
  parseFileItemsFromUriList,
} from '@/lib/clipboard-utils'

export function projectClipboardEntry(dto: ClipboardEntryDto): ClipboardEntry {
  const isFile = isFileContentType(dto.contentType)
  const isImage = !isFile && isImageContentType(dto.contentType)
  const isLink = !isFile && !isImage && (dto.linkUrls?.length ?? 0) > 0
  const contentTags = entryTags({
    contentType: dto.contentType,
    hasLink: isLink,
  })

  let type: ClipboardEntryType
  let content: ClipboardEntryContent | null
  if (isFile) {
    const parsed = parseFileItemsFromUriList(dto.preview)
    type = 'file'
    content = {
      file_names: parsed.map(p => p.name),
      file_sizes: dto.fileSizes ?? [],
      file_missing: parsed.map(p => p.missing),
      file_paths: parsed.map(p => p.path),
    }
  } else if (isImage) {
    type = 'image'
    content = {
      thumbnail: dto.thumbnailUrl ?? null,
      size: dto.sizeBytes,
      width: dto.imageWidth ?? 0,
      height: dto.imageHeight ?? 0,
    }
  } else if (isLink) {
    type = 'link'
    content = {
      urls: dto.linkUrls!,
      domains: dto.linkDomains ?? dto.linkUrls!.map(extractDomainFromUrl),
    }
  } else {
    type = 'text'
    content = {
      display_text: dto.preview,
      has_detail: dto.hasDetail,
      size: dto.sizeBytes,
    }
  }

  return {
    id: dto.id,
    type,
    ...(contentTags.length > 0 ? { contentTags } : {}),
    content,
    createdAt: dto.capturedAt,
    updatedAt: dto.updatedAt,
    activeTime: dto.activeTime,
    isFavorited: dto.isFavorited,
    isUnavailable: dto.payloadState === 'Lost',
  }
}

/** Map a backend content category to the display render type. `link` lives in
 * the tag dimension, not here. */
function searchContentTypeToDisplayType(ft: SearchResultDto['contentType']): ClipboardEntryType {
  switch (ft) {
    case 'text':
      return 'text'
    case 'html':
      return 'code'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'other':
      return 'unknown'
  }
}

function entryTags({
  contentType,
  hasLink,
  tags = [],
}: {
  contentType: string
  hasLink: boolean
  tags?: string[]
}): ClipboardEntryTag[] {
  const out: ClipboardEntryTag[] = []
  if (hasLink || tags.includes('link')) out.push('link')
  // Honor the backend `code` tag too: source-like plain text is tagged `code`
  // server-side even though its MIME type is text/plain, so deriving `code` only
  // from HTML would drop the code pill on those hits.
  if (contentType === 'html' || contentType === 'text/html' || tags.includes('code'))
    out.push('code')
  return out
}

/**
 * Project a search hit into a renderable history card — the search-side analogue
 * of {@link projectClipboardEntry}, and the single place search DTOs become
 * display items (shared by the history page and the dashboard).
 *
 * Builds just enough structured content (link/file/text) for native rendering
 * instead of a bare preview line. Lazy fields (image dimensions, per-file sizes)
 * are not indexed: the image card lazy-loads its thumbnail by entry id; file
 * sizes render as unknown. `link` is a derived tag — a text entry carrying web
 * URLs renders as a link card (§4.3).
 */
export function searchResultToDisplayItem(r: SearchResultDto): DisplayClipboardItem {
  const hasLink = r.linkUrls.length > 0
  let type = searchContentTypeToDisplayType(r.contentType)
  if (type === 'text' && hasLink) type = 'link'
  const contentTags = entryTags({
    contentType: r.contentType,
    hasLink,
    tags: r.tags,
  })

  let content: ClipboardEntryContent | null
  switch (type) {
    case 'link':
      content = { urls: r.linkUrls, domains: r.linkUrls.map(extractDomainFromUrl) }
      break
    case 'file':
      content = { file_names: r.fileNames, file_sizes: r.fileNames.map(() => -1) }
      break
    case 'image':
      // ImageCard resolves the thumbnail by entry id; no structured content.
      content = null
      break
    case 'code':
      content =
        r.textPreview != null ? { code: r.textPreview, char_count: r.charCount ?? undefined } : null
      break
    case 'text':
      content =
        r.textPreview != null
          ? {
              display_text: r.textPreview,
              has_detail: false,
              size: 0,
              char_count: r.charCount ?? undefined,
            }
          : null
      break
    default:
      content = null
  }

  return {
    id: r.entryId,
    type,
    ...(contentTags.length > 0 ? { contentTags } : {}),
    content,
    activeTime: r.activeTimeMs,
    isFavorited: r.tags.includes('favorited'),
    isUnavailable: r.payloadState === 'Lost',
    textPreview: r.textPreview ?? undefined,
  }
}
