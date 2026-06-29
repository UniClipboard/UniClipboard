import React from 'react'
import { useTranslation } from 'react-i18next'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { formatFileSize } from '@/utils'
import EntryDeliveryBadge from './EntryDeliveryBadge'

interface ClipboardPreviewInfoProps {
  imageDimensions: { width: number; height: number } | null
  item: DisplayClipboardItem | null
  preview: ClipboardPreviewData | null
  delivery: EntryDeliveryView | null
}

interface InfoRow {
  id: string
  value: React.ReactNode
}

function buildInfoRows(
  item: DisplayClipboardItem,
  preview: ClipboardPreviewData | null,
  imageDimensions: { width: number; height: number } | null,
  t: (key: string, options?: Record<string, unknown>) => string
): InfoRow[] {
  const rows: InfoRow[] = [{ id: 'type', value: t('header.filters.' + item.type) }]

  if (item.type === 'text' && item.content) {
    const textItem = item.content as ClipboardTextItem
    // Prefer the loaded full text; otherwise the indexed `char_count` (the true
    // length) rather than the capped preview, which would under-report.
    const fullText = preview?.contentType === 'text' ? preview.textContent : null
    const charCount =
      fullText != null ? fullText.length : (textItem.char_count ?? textItem.display_text.length)
    rows.push({
      id: 'text-chars',
      value: t('clipboard.preview.charactersCount', { count: charCount }),
    })
    if (textItem.size > 0) rows.push({ id: 'text-size', value: formatFileSize(textItem.size) })
  }

  if (item.type === 'code' && item.content) {
    const codeItem = item.content as ClipboardCodeItem
    const fullCode = preview?.contentType === 'text' ? preview.textContent : null
    const charCount =
      fullCode != null ? fullCode.length : (codeItem.char_count ?? codeItem.code.length)
    rows.push({
      id: 'code-chars',
      value: t('clipboard.preview.charactersCount', { count: charCount }),
    })
  }

  if (item.type === 'image' && item.content) {
    const imageItem = item.content as ClipboardImageItem
    const dims =
      imageDimensions ??
      (imageItem.width > 0 ? { width: imageItem.width, height: imageItem.height } : null)
    if (dims) rows.push({ id: 'image-dims', value: `${dims.width} × ${dims.height}` })
    if (imageItem.size > 0) rows.push({ id: 'image-size', value: formatFileSize(imageItem.size) })
  }

  if (item.type === 'file' && item.content) {
    const fileItem = item.content as ClipboardFileItem
    rows.push({
      id: 'file-count',
      value: t('clipboard.preview.filesCount', { count: fileItem.file_names.length }),
    })
    const knownSizes = fileItem.file_sizes.filter(size => size >= 0)
    if (knownSizes.length > 0) {
      const totalSize = knownSizes.reduce((sum, size) => sum + size, 0)
      rows.push({ id: 'file-size', value: formatFileSize(totalSize) })
    }
  }

  if (item.type === 'link' && item.content) {
    const linkItem = item.content as ClipboardLinkItem
    const uniqueDomains = [...new Set(linkItem.domains.filter(Boolean))]
    if (uniqueDomains.length > 0) rows.push({ id: 'link-domain', value: uniqueDomains[0] })
    rows.push({
      id: 'link-chars',
      value: t('clipboard.preview.charactersCount', { count: linkItem.urls[0]?.length ?? 0 }),
    })
  }

  return rows
}

/**
 * Lightweight meta strip atop the preview pane: a single dot-separated line of
 * facts (type · size · dims …) on the same `bg-card` surface as the body, with
 * the delivery badge pinned right. No background block or divider — the preview
 * reads as one continuous surface, with hierarchy carried by type scale and
 * spacing rather than rules.
 */
const ClipboardPreviewInfo: React.FC<ClipboardPreviewInfoProps> = ({
  imageDimensions,
  item,
  preview,
  delivery,
}) => {
  const { t } = useTranslation()

  if (!item) return null

  const rows = buildInfoRows(item, preview, imageDimensions, t)

  if (rows.length === 0 && !delivery) return null

  return (
    <div className="shrink-0 px-6 pt-4 pb-2">
      <div className="flex items-center gap-2 text-[11px] font-medium tabular-nums text-muted-foreground/55">
        {rows.map((row, i) => (
          <React.Fragment key={row.id}>
            {i > 0 && <span className="text-muted-foreground/25">·</span>}
            <span className="shrink-0">{row.value}</span>
          </React.Fragment>
        ))}
        {delivery && (
          <div className="ml-auto">
            <EntryDeliveryBadge delivery={delivery} />
          </div>
        )}
      </div>
    </div>
  )
}

export default ClipboardPreviewInfo
