import {
  Cloud,
  Code,
  ExternalLink,
  File,
  FileText,
  History,
  Image as ImageIcon,
  Laptop,
} from 'lucide-react'
import type React from 'react'
import type { EntrySourceView } from '@/api/tauri-command/clipboard_delivery'
import type {
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { formatFileSize } from '@/utils'

export const TYPE_ICONS: Record<string, React.ElementType> = {
  text: FileText,
  code: Code,
  link: ExternalLink,
  richtext: FileText,
  image: ImageIcon,
  file: File,
  unknown: FileText,
}

const SOURCE_CONFIG: Record<EntrySourceView['tag'], { icon: React.ElementType; color: string }> = {
  local: { icon: Laptop, color: 'text-muted-foreground/40' },
  remote: { icon: Cloud, color: 'text-sky-500/60' },
  historical: { icon: History, color: 'text-muted-foreground/30' },
}

export interface SourceDescription {
  Icon: React.ElementType
  color: string
  label: string | null
}

type Translate = (key: string, opts?: Record<string, unknown>) => string

export function fileNameFromPreview(preview: string): string {
  const trimmed = preview.trim().replace(/[/\\]+$/, '')
  const segment = trimmed.split(/[/\\]/).pop() ?? trimmed
  try {
    return decodeURIComponent(segment)
  } catch {
    return segment
  }
}

export function getContentSizeLabel(item: DisplayClipboardItem, t: Translate): string | null {
  if (!item.content) return null
  switch (item.type) {
    case 'text': {
      const textItem = item.content as ClipboardTextItem
      const count = textItem.char_count ?? textItem.display_text.length
      return t('clipboard.preview.charactersCount', { count })
    }
    case 'richtext': {
      const textItem = item.content as ClipboardTextItem
      const count = textItem.char_count ?? textItem.display_text.length
      return t('clipboard.preview.charactersCount', { count })
    }
    case 'image':
      return null
    case 'file': {
      const file = item.content as ClipboardFileItem
      const count = file.file_names.length
      if (count > 1) return t('clipboard.preview.filesCount', { count })
      const totalSize = file.file_sizes.filter(s => s >= 0).reduce((a, b) => a + b, 0)
      return totalSize > 0 ? formatFileSize(totalSize) : null
    }
    default:
      return null
  }
}

export function describeSource(
  source: EntrySourceView,
  t: (key: string) => string
): SourceDescription {
  const cfg = SOURCE_CONFIG[source.tag]
  if (source.tag === 'remote') {
    return {
      Icon: cfg.icon,
      color: cfg.color,
      label: source.deviceName ?? source.deviceId.slice(0, 6),
    }
  }
  if (source.tag === 'local') {
    return { Icon: cfg.icon, color: cfg.color, label: t('clipboard.source.local') }
  }
  return { Icon: cfg.icon, color: cfg.color, label: null }
}

export function imageTitle(
  label: string,
  loadedDims: { w: number; h: number } | null,
  imageItem?: ClipboardImageItem | null
): string {
  const width = loadedDims?.w ?? (imageItem && imageItem.width > 0 ? imageItem.width : 0)
  const height = loadedDims?.h ?? (imageItem && imageItem.height > 0 ? imageItem.height : 0)
  return width > 0 && height > 0 ? `${label} (${width}×${height})` : label
}
