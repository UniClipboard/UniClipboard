import { fetchClipboardResourceText, resolveResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryDetail, getClipboardEntryResource } from '@/api/daemon/clipboard'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { parseFileNamesFromUriList } from '@/lib/clipboard-utils'

export async function loadClipboardPreview(entryId: string): Promise<ClipboardPreviewData | null> {
  const resource = await getClipboardEntryResource(entryId)
  if (!resource) {
    return null
  }

  if (resource.mimeType === 'image' || resource.mimeType.startsWith('image/')) {
    return {
      entryId,
      contentType: 'image',
      sizeBytes: resource.sizeBytes,
      imageUrl: resolveResourceImageUrl(resource) ?? undefined,
    }
  }

  if (resource.mimeType.includes('uri-list')) {
    const detail = await getClipboardEntryDetail(entryId)
    if (!detail) {
      return null
    }

    return {
      entryId,
      contentType: 'file',
      sizeBytes: resource.sizeBytes,
      fileNames: parseFileNamesFromUriList(detail.content),
    }
  }

  const textContent = await fetchClipboardResourceText(resource)
  return {
    entryId,
    contentType: 'text',
    sizeBytes: resource.sizeBytes,
    textContent,
  }
}
