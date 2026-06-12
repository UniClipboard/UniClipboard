/**
 * Daemon DTO → UI domain projection.
 *
 * The single place where the daemon's `EntryProjectionDto` wire shape is
 * mapped to the `ClipboardEntry` domain model. Every list/event path
 * (`clipboardSlice`, `useClipboardCollection`, `useClipboardEventStream`)
 * goes through this function, so daemon field changes only impact this file.
 */
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import type {
  ClipboardEntry,
  ClipboardEntryContent,
  ClipboardEntryType,
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

  let type: ClipboardEntryType
  let content: ClipboardEntryContent | null
  if (isFile) {
    const parsed = parseFileItemsFromUriList(dto.preview)
    type = 'file'
    content = {
      file_names: parsed.map(p => p.name),
      file_sizes: dto.fileSizes ?? [],
      file_missing: parsed.map(p => p.missing),
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
    content,
    createdAt: dto.capturedAt,
    updatedAt: dto.updatedAt,
    activeTime: dto.activeTime,
    isFavorited: dto.isFavorited,
    isUnavailable: dto.payloadState === 'Lost',
  }
}
