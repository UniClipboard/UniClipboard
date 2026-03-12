import { Code, ExternalLink, File, FileText, Image as ImageIcon } from 'lucide-react'
import React from 'react'
import type { DisplayClipboardItem } from './ClipboardContent'
import {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
} from '@/api/clipboardItems'
import { cn } from '@/lib/utils'

interface ClipboardItemRowProps {
  item: DisplayClipboardItem
  isActive: boolean
  onClick: () => void
  itemRef?: React.Ref<HTMLDivElement>
}

const typeIcons: Record<DisplayClipboardItem['type'], React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  link: ExternalLink,
  code: Code,
  file: File,
  unknown: FileText,
}

function getPreviewText(item: DisplayClipboardItem): string {
  if (!item.content) return ''
  switch (item.type) {
    case 'text':
      return (item.content as ClipboardTextItem).display_text.slice(0, 80)
    case 'image': {
      const img = item.content as ClipboardImageItem
      if (img.width > 0 && img.height > 0) {
        return `Image(${img.width}x${img.height})`
      }
      return 'Image'
    }
    case 'link':
      return (item.content as ClipboardLinkItem).urls[0] ?? ''
    case 'code':
      return (item.content as ClipboardCodeItem).code.split('\n')[0] ?? ''
    case 'file': {
      const names = (item.content as ClipboardFileItem).file_names
      return names.length > 0 ? names.join(', ') : 'File'
    }
    default:
      return ''
  }
}

const ClipboardItemRow: React.FC<ClipboardItemRowProps> = ({
  item,
  isActive,
  onClick,
  itemRef,
}) => {
  const Icon = typeIcons[item.type] ?? FileText

  return (
    <div
      ref={itemRef}
      className={cn(
        'flex items-center gap-3 py-2.5 px-3 rounded-lg cursor-pointer select-none transition-colors shrink-0 overflow-hidden',
        isActive ? 'bg-primary/10 text-foreground' : 'hover:bg-muted/50 text-foreground/80'
      )}
      onClick={onClick}
    >
      <Icon
        className={cn('h-4 w-4 shrink-0', isActive ? 'text-primary' : 'text-muted-foreground')}
      />
      <span className="w-0 flex-grow truncate text-sm">{getPreviewText(item)}</span>
      {item.type === 'link' &&
        item.content &&
        (item.content as ClipboardLinkItem).urls.length > 1 && (
          <span className="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded-full shrink-0">
            +{(item.content as ClipboardLinkItem).urls.length - 1}
          </span>
        )}
      <span className="text-xs text-muted-foreground shrink-0">{item.time}</span>
    </div>
  )
}

export default ClipboardItemRow
