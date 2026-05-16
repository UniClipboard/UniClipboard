import { Database, Files, Globe, Hash, Layers, Maximize, Type } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
} from '@/api/clipboardItems'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { formatFileSize } from '@/utils'
import type { DisplayClipboardItem } from './ClipboardContent'
import EntryDeliveryBadge from './EntryDeliveryBadge'

interface ClipboardPreviewInfoProps {
  imageDimensions: { width: number; height: number } | null
  item: DisplayClipboardItem | null
  preview: ClipboardPreviewData | null
  delivery: EntryDeliveryView | null
}

interface InfoRow {
  icon: React.ElementType
  value: React.ReactNode
}

function buildInfoRows(
  item: DisplayClipboardItem,
  preview: ClipboardPreviewData | null,
  imageDimensions: { width: number; height: number } | null,
  t: (key: string, options?: Record<string, unknown>) => string
): InfoRow[] {
  const rows: InfoRow[] = [{ icon: Layers, value: t('header.filters.' + item.type) }]

  if (item.type === 'text' && item.content) {
    const textItem = item.content as ClipboardTextItem
    const text =
      preview?.contentType === 'text' ? (preview.textContent ?? '') : textItem.display_text
    rows.push({
      icon: Type,
      value: t('clipboard.preview.charactersCount', { count: text.length }),
    })
    if (textItem.size > 0) rows.push({ icon: Database, value: formatFileSize(textItem.size) })
  }

  if (item.type === 'code' && item.content) {
    const code =
      preview?.contentType === 'text'
        ? (preview.textContent ?? (item.content as ClipboardCodeItem).code)
        : (item.content as ClipboardCodeItem).code
    rows.push({
      icon: Type,
      value: t('clipboard.preview.charactersCount', { count: code.length }),
    })
  }

  if (item.type === 'image' && item.content) {
    const imageItem = item.content as ClipboardImageItem
    const dims =
      imageDimensions ??
      (imageItem.width > 0 ? { width: imageItem.width, height: imageItem.height } : null)
    if (dims) rows.push({ icon: Maximize, value: `${dims.width} × ${dims.height}` })
    if (imageItem.size > 0) rows.push({ icon: Database, value: formatFileSize(imageItem.size) })
  }

  if (item.type === 'file' && item.content) {
    const fileItem = item.content as ClipboardFileItem
    rows.push({
      icon: Files,
      value: t('clipboard.preview.filesCount', { count: fileItem.file_names.length }),
    })
    const knownSizes = fileItem.file_sizes.filter(size => size >= 0)
    if (knownSizes.length > 0) {
      const totalSize = knownSizes.reduce((sum, size) => sum + size, 0)
      rows.push({ icon: Database, value: formatFileSize(totalSize) })
    }
  }

  if (item.type === 'link' && item.content) {
    const linkItem = item.content as ClipboardLinkItem
    const uniqueDomains = [...new Set(linkItem.domains.filter(Boolean))]
    if (uniqueDomains.length > 0) rows.push({ icon: Globe, value: uniqueDomains[0] })
    rows.push({
      icon: Hash,
      value: t('clipboard.preview.charactersCount', { count: linkItem.urls[0]?.length ?? 0 }),
    })
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
    <div className="shrink-0 overflow-hidden bg-muted/10 px-6 py-3">
      <div className="flex items-center gap-6">
        {rows.map((row, index) => (
          <div key={index} className="group flex shrink-0 items-center gap-2">
            <row.icon className="h-3.5 w-3.5 text-muted-foreground/20 transition-colors group-hover:text-primary/50" />
            <span className="text-[11px] font-semibold tabular-nums text-muted-foreground/60">
              {row.value}
            </span>
          </div>
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
