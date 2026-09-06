import React from 'react'
import { useTranslation } from 'react-i18next'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import type {
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { formatFileSize } from '@/utils'
import EntryDeliveryBadge from './EntryDeliveryBadge'
import { countCodeLines, resolveCodePreviewText } from './preview-renderers/codePreviewUtils'

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

  if ((item.type === 'text' || item.type === 'richtext') && item.content) {
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
    if (item.contentTags?.includes('code')) {
      const code = resolveCodePreviewText(textItem.display_text, preview)
      rows.push({
        id: 'code-lines',
        value: t('clipboard.preview.linesCount', { count: countCodeLines(code) }),
      })
    }
    if (textItem.size > 0) rows.push({ id: 'text-size', value: formatFileSize(textItem.size) })
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

  return rows
}

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
    <div className="shrink-0 p-3" data-testid="clipboard-preview-info">
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1.5 text-[0.6875rem] font-normal tabular-nums text-muted-foreground/75">
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          {rows.map((row, i) => (
            <span key={row.id} className="inline-flex items-center gap-2">
              {i > 0 && (
                <span aria-hidden="true" className="text-muted-foreground/40">
                  ·
                </span>
              )}
              <span>{row.value}</span>
            </span>
          ))}
          {item.contentTags?.map(tag => (
            <span
              key={tag}
              className="rounded bg-muted/50 px-1.5 py-0.5 text-[0.625rem] leading-none text-muted-foreground/80"
            >
              {t(`history.type.${tag}`, tag)}
            </span>
          ))}
        </div>
        {delivery && (
          <div className="min-w-0 max-w-full [&>div]:flex-wrap [&_span]:font-normal">
            <EntryDeliveryBadge delivery={delivery} />
          </div>
        )}
      </div>
    </div>
  )
}

export default ClipboardPreviewInfo
