import type { ClipboardTextItem } from '@/api/clipboardItems'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

export const LARGE_TEXT_THRESHOLD = 50_000

export function getTextPreviewContent(
  item: ClipboardTextItem,
  preview: ClipboardPreviewData | null
) {
  return preview?.contentType === 'text' ? (preview.textContent ?? '') : item.display_text
}

export function isLargeTextPreview(
  item: ClipboardTextItem,
  preview: ClipboardPreviewData | null,
  loading: boolean
): boolean {
  return !loading && getTextPreviewContent(item, preview).length > LARGE_TEXT_THRESHOLD
}
