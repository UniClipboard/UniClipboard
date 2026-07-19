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
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import {
  isFileContentType,
  isImageContentType,
  parseFileItemsFromUriList,
} from '@/lib/clipboard-utils'

export function projectClipboardEntry(dto: ClipboardEntryDto): ClipboardEntry {
  const isFile = isFileContentType(dto.contentType)
  const isImage = !isFile && isImageContentType(dto.contentType)
  const isRichText = !isFile && !isImage && isRichTextContentType(dto.contentType)
  const hasLink = !isFile && !isImage && (dto.linkUrls?.length ?? 0) > 0
  const contentTags = entryTags({
    hasLink,
    tags: dto.contentTags ?? [],
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
  } else if (isRichText) {
    type = 'richtext'
    content = withLinkMetadata(
      {
        display_text: dto.preview,
        has_detail: dto.hasDetail,
        size: dto.sizeBytes,
      },
      dto.linkUrls,
      dto.linkDomains
    )
  } else {
    // Backend physical category `other` has no display type of its own;
    // surface it as `unknown` rather than masquerading as `text` (mirrors
    // searchContentTypeToDisplayType's default for `other`).
    type = dto.contentType.toLowerCase() === 'other' ? 'unknown' : 'text'
    content = withLinkMetadata(
      {
        display_text: dto.preview,
        has_detail: dto.hasDetail,
        size: dto.sizeBytes,
      },
      dto.linkUrls,
      dto.linkDomains
    )
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
    isDirectory: dto.isDirectory ?? false,
  }
}

/**
 * Project a browse-side {@link ClipboardEntry} into the {@link DisplayClipboardItem}
 * the live list renders — the browse analogue of {@link searchResultToDisplayItem}.
 *
 * `ClipboardEntry` is a structural superset of the item, but the display model
 * intentionally carries only the render-relevant subset (no `createdAt`/
 * `updatedAt`). Centralizing the field copy here keeps every new entry field a
 * one-line decision — a hand-maintained inline literal silently dropped
 * `isDirectory` before, killing the directory-send status row (issue #1330).
 */
export function clipboardEntryToDisplayItem(entry: ClipboardEntry): DisplayClipboardItem {
  return {
    id: entry.id,
    type: entry.type,
    contentTags: entry.contentTags,
    content: entry.content,
    activeTime: entry.activeTime,
    isFavorited: entry.isFavorited,
    isUnavailable: entry.isUnavailable,
    isDirectory: entry.isDirectory,
  }
}

function withLinkMetadata(
  item: ClipboardTextItem,
  linkUrls: string[] | null | undefined,
  linkDomains: string[] | null | undefined
): ClipboardTextItem {
  if (!linkUrls || linkUrls.length === 0) return item
  return {
    ...item,
    link_urls: linkUrls,
    ...(linkDomains && linkDomains.length > 0 ? { link_domains: linkDomains } : {}),
  }
}

/** Map a backend content category to the display render type. `link` lives in
 * the tag dimension, not here. */
function searchContentTypeToDisplayType(ft: SearchResultDto['contentType']): ClipboardEntryType {
  switch (ft) {
    case 'text':
      return 'text'
    case 'html':
      return 'richtext'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'other':
      return 'unknown'
  }
}

function entryTags({
  hasLink,
  tags = [],
}: {
  hasLink: boolean
  tags?: string[]
}): ClipboardEntryTag[] {
  const out: ClipboardEntryTag[] = []
  if (hasLink || tags.includes('link')) out.push('link')
  // Honor the backend `code` tag too: source-like plain text is tagged `code`
  // server-side even though its MIME type is text/plain.
  if (tags.includes('code')) out.push('code')
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
  const type = searchContentTypeToDisplayType(r.contentType)
  const contentTags = entryTags({
    hasLink,
    tags: r.tags,
  })

  let content: ClipboardEntryContent | null
  switch (type) {
    case 'file':
      content = {
        file_names: r.fileNames,
        file_sizes: r.fileNames.map(() => -1),
        // Local paths captured at index time (search-v9), aligned with fileNames.
        // Empty when no file resolves to a local path; backs "open file location".
        file_paths: r.filePaths,
      }
      break
    case 'image':
      // ImageCard resolves the thumbnail by entry id; no structured content.
      content = null
      break
    case 'richtext':
    case 'text':
      content =
        r.textPreview != null
          ? withLinkMetadata(
              {
                display_text: r.textPreview,
                has_detail: false,
                size: 0,
                char_count: r.charCount ?? undefined,
              },
              r.linkUrls,
              null
            )
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
    // Reuse the builtin `directory` tag the search projection already derives
    // (evaluate_builtin_content_tags), so search rows key off the same
    // single-source directory signal the browse projection carries via
    // `isDirectory`. Drives the status-only send row for directory sends.
    isDirectory: r.tags.includes('directory'),
    textPreview: r.textPreview ?? undefined,
  }
}

function isRichTextContentType(contentType: string): boolean {
  const normalized = contentType.toLowerCase()
  return (
    normalized === 'rich_text' || normalized === 'richtext' || normalized.startsWith('text/html')
  )
}
