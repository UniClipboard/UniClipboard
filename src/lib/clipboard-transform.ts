/**
 * Shared daemon DTO → frontend ClipboardItemResponse transformer.
 *
 * Consolidates the transformation logic that was previously duplicated across
 * clipboardSlice, useClipboardCollection, and useClipboardEventStream.
 */
import type { ClipboardItemResponse } from '@/api/clipboardItems'
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import {
  extractDomainFromUrl,
  isFileContentType,
  isImageContentType,
  parseFileItemsFromUriList,
} from '@/lib/clipboard-utils'

export function transformDaemonDtoToItemResponse(entry: ClipboardEntryDto): ClipboardItemResponse {
  const isFile = isFileContentType(entry.contentType)
  const isImage = !isFile && isImageContentType(entry.contentType)
  const hasLinkData = !isImage && entry.linkUrls && entry.linkUrls.length > 0

  let linkItem: { urls: string[]; domains: string[] } | null = null
  if (hasLinkData) {
    linkItem = {
      urls: entry.linkUrls!,
      domains: entry.linkDomains ?? entry.linkUrls!.map(extractDomainFromUrl),
    }
  }

  return {
    id: entry.id,
    is_downloaded: true,
    is_favorited: entry.isFavorited,
    created_at: entry.capturedAt,
    updated_at: entry.updatedAt,
    active_time: entry.activeTime,
    item: {
      text:
        !isImage && !isFile && !hasLinkData
          ? { display_text: entry.preview, has_detail: entry.hasDetail, size: entry.sizeBytes }
          : null,
      image: isImage
        ? {
            thumbnail: entry.thumbnailUrl ?? null,
            size: entry.sizeBytes,
            width: entry.imageWidth ?? 0,
            height: entry.imageHeight ?? 0,
          }
        : null,
      file: isFile
        ? (() => {
            const parsed = parseFileItemsFromUriList(entry.preview)
            return {
              file_names: parsed.map(p => p.name),
              file_sizes: entry.fileSizes ?? [],
              file_missing: parsed.map(p => p.missing),
            }
          })()
        : null,
      link: linkItem as unknown as ClipboardItemResponse['item']['link'],
      code: null,
      unknown: null,
    },
    file_transfer_status: entry.fileTransferStatus ?? null,
    file_transfer_reason: entry.fileTransferReason ?? null,
    payload_state: entry.payloadState ?? null,
  }
}
