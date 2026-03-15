import {
  AlertCircle,
  Clock,
  Code,
  ExternalLink,
  File,
  FileArchive,
  FileImage,
  FileMusic,
  FileSpreadsheet,
  FileText,
  FileType,
  Image as ImageIcon,
  Loader2,
} from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type { DisplayClipboardItem } from './ClipboardContent'
import TransferProgressBar from './TransferProgressBar'
import {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
} from '@/api/clipboardItems'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import {
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'

interface ClipboardItemRowProps extends React.HTMLAttributes<HTMLDivElement> {
  item: DisplayClipboardItem
  isActive: boolean
  isStale?: boolean
  onClick: () => void
  elementRef?: React.Ref<HTMLDivElement>
}

const FILE_EXT_ICON_MAP: Record<string, React.ElementType> = {
  // Images
  jpg: FileImage,
  jpeg: FileImage,
  png: FileImage,
  gif: FileImage,
  bmp: FileImage,
  svg: FileImage,
  webp: FileImage,
  // Archives
  zip: FileArchive,
  rar: FileArchive,
  '7z': FileArchive,
  tar: FileArchive,
  gz: FileArchive,
  // Documents
  doc: FileSpreadsheet,
  docx: FileSpreadsheet,
  xls: FileSpreadsheet,
  xlsx: FileSpreadsheet,
  ppt: FileSpreadsheet,
  pptx: FileSpreadsheet,
  // PDF
  pdf: FileType,
  // Audio
  mp3: FileMusic,
  wav: FileMusic,
  flac: FileMusic,
  aac: FileMusic,
}

const typeIcons: Record<DisplayClipboardItem['type'], React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  link: ExternalLink,
  code: Code,
  file: File,
  unknown: FileText,
}

/**
 * Extract the file extension from the first file name of a file item for icon lookup.
 */
function getFileExt(item: DisplayClipboardItem): string {
  if (item.type !== 'file' || !item.content) return ''
  const firstName = (item.content as ClipboardFileItem).file_names[0] ?? ''
  return firstName.split('.').pop()?.toLowerCase() ?? ''
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
      const fileContent = item.content as ClipboardFileItem
      const names = fileContent.file_names
      if (names.length === 0) return 'File'
      if (names.length === 1) return names[0]
      return `${names.length} files`
    }
    default:
      return ''
  }
}

const ClipboardItemRow = React.forwardRef<HTMLDivElement, ClipboardItemRowProps>(
  ({ item, isActive, isStale, onClick, elementRef, className: extraClassName, ...rest }, ref) => {
    const { t } = useTranslation()
    const Icon = FILE_EXT_ICON_MAP[getFileExt(item)] ?? typeIcons[item.type] ?? FileText
    const transfer = useAppSelector(state => selectTransferByEntryId(state, item.id))
    const entryStatus = useAppSelector(state => selectEntryTransferStatus(state, item.id))

    // Derive display state: durable entryStatus takes priority, fall back to ephemeral transfer
    const isFile = item.type === 'file'
    const durableStatus = entryStatus?.status
    const isTransferring =
      durableStatus === 'transferring' || (transfer?.status === 'active' && !durableStatus)
    const isTransferFailed =
      durableStatus === 'failed' || (transfer?.status === 'failed' && !durableStatus)
    const isPending = durableStatus === 'pending'

    return (
      <div
        ref={elementRef ?? ref}
        {...rest}
        className={cn(
          'flex flex-col gap-1 py-2.5 px-3 rounded-lg cursor-pointer select-none transition-colors shrink-0 overflow-hidden',
          isActive ? 'bg-primary/10 text-foreground' : 'hover:bg-muted/50 text-foreground/80',
          isTransferring && 'ring-1 ring-primary/20',
          isTransferFailed && 'ring-1 ring-destructive/20',
          isPending && 'ring-1 ring-muted-foreground/20',
          extraClassName
        )}
        onClick={onClick}
      >
        <div className="flex items-center gap-3">
          <Icon
            className={cn(
              'h-4 w-4 shrink-0',
              isActive ? 'text-primary' : 'text-muted-foreground',
              isStale && 'opacity-40',
              isPending && 'opacity-50'
            )}
          />
          <span
            className={cn(
              'w-0 flex-grow truncate text-sm',
              isStale && 'text-muted-foreground line-through opacity-60',
              isPending && 'text-muted-foreground opacity-70'
            )}
          >
            {getPreviewText(item)}
          </span>
          {item.type === 'link' &&
            item.content &&
            (item.content as ClipboardLinkItem).urls.length > 1 && (
              <span className="text-xs text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded-full shrink-0">
                +{(item.content as ClipboardLinkItem).urls.length - 1}
              </span>
            )}
          {isFile && isPending && (
            <TooltipProvider delayDuration={0}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Clock
                    className="h-3.5 w-3.5 text-muted-foreground shrink-0"
                    aria-label={t('clipboard.transfer.statusBadge.pending')}
                  />
                </TooltipTrigger>
                <TooltipContent side="left">
                  <p className="text-xs">{t('clipboard.transfer.pending')}</p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {isFile && isTransferring && (
            <TooltipProvider delayDuration={0}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Loader2
                    className="h-3.5 w-3.5 text-primary animate-spin shrink-0"
                    aria-label={t('clipboard.transfer.statusBadge.transferring')}
                  />
                </TooltipTrigger>
                <TooltipContent side="left">
                  <p className="text-xs">{t('clipboard.transfer.transferring')}</p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {isTransferFailed ? (
            <TooltipProvider delayDuration={0}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <AlertCircle
                    className="h-3.5 w-3.5 text-destructive shrink-0"
                    aria-label={t('clipboard.transfer.statusBadge.failed')}
                  />
                </TooltipTrigger>
                <TooltipContent side="left">
                  <p className="text-xs">
                    {entryStatus?.reason ||
                      transfer?.errorMessage ||
                      t('clipboard.transfer.failed')}
                  </p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          ) : (
            !isPending &&
            !isTransferring && (
              <span className="text-xs text-muted-foreground shrink-0">{item.time}</span>
            )
          )}
        </div>
        {isTransferring && transfer && (
          <div className="pl-7">
            <TransferProgressBar progress={transfer} variant="compact" />
          </div>
        )}
      </div>
    )
  }
)

ClipboardItemRow.displayName = 'ClipboardItemRow'

export default ClipboardItemRow
