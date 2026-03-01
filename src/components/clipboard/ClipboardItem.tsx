import {
  ChevronDown,
  ChevronUp,
  File,
  ExternalLink,
  Image as ImageIcon,
  Loader2,
} from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ClipboardTextItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardCodeItem,
  ClipboardFileItem,
  fetchClipboardResourceText,
  getClipboardEntryResource,
} from '@/api/clipboardItems'
import { toast } from '@/components/ui/toast'
import { cn } from '@/lib/utils'
import { formatFileSize } from '@/utils'

interface ClipboardItemProps {
  index: number
  type: 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'
  time: string
  device?: string
  content:
    | ClipboardTextItem
    | ClipboardImageItem
    | ClipboardLinkItem
    | ClipboardCodeItem
    | ClipboardFileItem
    | null
  entryId: string // NEW: need entry ID for detail fetch
  isSelected?: boolean
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void
  fileSize?: number
}

const ClipboardItem: React.FC<ClipboardItemProps> = ({
  index,
  type,
  time,
  content,
  entryId,
  isSelected = false,
  onSelect,
  fileSize,
}) => {
  const { t } = useTranslation()
  const [isExpanded, setIsExpanded] = useState(false)
  const [detailContent, setDetailContent] = useState<string | null>(null)
  const [detailImageUrl, setDetailImageUrl] = useState<string | null>(null)
  const [isLoadingDetail, setIsLoadingDetail] = useState(false)

  // Determine if expand button should show (based on UI display needs)
  const shouldShowExpandButton = (): boolean => {
    if (!content) return false

    switch (type) {
      case 'text': {
        const textItem = content as ClipboardTextItem
        // Show expand button if text is long (e.g., more than ~250 chars for 5 lines)
        // This is a UI decision, not based on has_detail
        return textItem.display_text.length > 250 || textItem.display_text.split('\n').length > 5
      }
      case 'image':
        return true
      case 'code':
        return (content as ClipboardCodeItem).code.split('\n').length > 6
      case 'link':
      case 'file':
      default:
        return false
    }
  }

  // Handle expand toggle
  const handleExpand = async () => {
    if (isExpanded) {
      // Already expanded: collapse
      setIsExpanded(false)
      return
    }

    if (type === 'text') {
      if (detailContent) {
        setIsExpanded(true)
        return
      }

      const textItem = content as ClipboardTextItem
      if (!textItem?.has_detail) {
        setIsExpanded(true)
        return
      }

      setIsLoadingDetail(true)
      try {
        const resource = await getClipboardEntryResource(entryId)
        const fullText = await fetchClipboardResourceText(resource)
        setDetailContent(fullText)
        setIsExpanded(true)
      } catch (e) {
        console.error('Failed to load detail:', e)
        toast.error(t('clipboard.errors.loadDetailFailed'), {
          description: e instanceof Error ? e.message : t('clipboard.errors.unknown'),
        })
      } finally {
        setIsLoadingDetail(false)
      }
      return
    }

    if (type === 'image') {
      if (detailImageUrl) {
        setIsExpanded(true)
        return
      }

      setIsLoadingDetail(true)
      try {
        const resource = await getClipboardEntryResource(entryId)
        setDetailImageUrl(resource.url)
        setIsExpanded(true)
      } catch (e) {
        console.error('Failed to load image detail:', e)
        toast.error(t('clipboard.errors.loadDetailFailed'), {
          description: e instanceof Error ? e.message : t('clipboard.errors.unknown'),
        })
      } finally {
        setIsLoadingDetail(false)
      }
      return
    }

    setIsExpanded(true)
  }

  // Calculate character count or size info
  const getSizeInfo = (): string => {
    if (!content) return ''
    switch (type) {
      case 'text':
        return `${(content as ClipboardTextItem).display_text.length} ${t('clipboard.item.characters')}`
      case 'link':
        return t('clipboard.item.link')
      case 'code':
        return `${(content as ClipboardCodeItem).code.length} ${t('clipboard.item.characters')}`
      case 'file':
        return formatFileSize(fileSize)
      case 'image':
        // Note: Use actual dimensions if available in API, otherwise placeholder or remove
        return t('clipboard.item.image')
      default:
        return ''
    }
  }

  const renderContent = () => {
    switch (type) {
      case 'text': {
        const textItem = content as ClipboardTextItem
        // Use detail content when expanded and available, otherwise use preview
        const textToShow = isExpanded && detailContent ? detailContent : textItem.display_text

        return (
          <p
            className={cn(
              'whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90 wrap-break-word',
              !isExpanded && 'line-clamp-5'
            )}
          >
            {isLoadingDetail ? t('clipboard.item.loading') : textToShow}
          </p>
        )
      }
      case 'image': {
        const thumbnailUrl = (content as ClipboardImageItem | null)?.thumbnail ?? null
        const imageUrl = isExpanded && detailImageUrl ? detailImageUrl : thumbnailUrl
        return (
          <div className="flex justify-center bg-black/20 rounded-lg overflow-hidden py-4">
            {imageUrl ? (
              <img
                src={imageUrl}
                className={cn(
                  'w-auto object-contain rounded-md shadow-sm transition-all duration-300',
                  isExpanded ? 'max-h-[32rem]' : 'h-32'
                )}
                alt={t('clipboard.item.altText.clipboardImage')}
                loading="lazy"
              />
            ) : (
              <div className="flex flex-col items-center justify-center gap-2 h-32 w-full rounded-md bg-muted/30 border border-border/30">
                <ImageIcon className="h-6 w-6 text-muted-foreground/70" />
                <span className="text-xs text-muted-foreground/70">
                  {t('clipboard.item.loading')}
                </span>
              </div>
            )}
          </div>
        )
      }
      case 'link': {
        const url = (content as ClipboardLinkItem).url
        return (
          <div className="flex flex-col gap-1">
            <a
              href={url}
              target="_blank"
              rel="noreferrer"
              className="text-primary font-medium hover:underline break-all text-sm leading-relaxed flex items-center gap-2"
              onClick={e => e.stopPropagation()}
            >
              <ExternalLink size={14} />
              {url}
            </a>
          </div>
        )
      }
      case 'code':
        return (
          <div className="bg-muted/30 p-3 rounded-lg border border-border/30 overflow-hidden font-mono text-xs">
            <pre
              className={cn(
                'whitespace-pre-wrap break-all text-foreground/80',
                !isExpanded && 'line-clamp-6'
              )}
            >
              {(content as ClipboardCodeItem).code}
            </pre>
          </div>
        )
      case 'file': {
        const fileNames = (content as ClipboardFileItem).file_names
        return (
          <div className="flex flex-col gap-2">
            {fileNames.map((name, i) => (
              <div key={i} className="flex items-center gap-2 text-sm text-foreground/80">
                <File size={16} className="text-muted-foreground" />
                <span className="truncate">{name}</span>
              </div>
            ))}
          </div>
        )
      }
      default:
        return <p className="text-muted-foreground text-sm">{t('clipboard.item.unknownContent')}</p>
    }
  }

  return (
    <div
      className={cn(
        'group relative flex flex-col border-b border-border/40 transition-all duration-300 select-none',
        isSelected
          ? 'bg-primary/5 border-l-4 border-l-primary'
          : 'hover:bg-muted/20 border-l-4 border-l-transparent hover:border-l-primary/30'
      )}
      onClick={e => {
        const selection = window.getSelection()
        if (selection && selection.toString().length > 0) {
          return
        }
        onSelect?.(e)
      }}
    >
      {/* Main Content Area */}
      <div className="select-text p-4">{renderContent()}</div>

      {/* Footer Area */}
      <div className="flex items-center justify-between px-4 pb-2 pt-1 text-xs text-muted-foreground/60 select-none">
        {/* Left: Time */}
        <div className="min-w-20">{time}</div>

        {/* Center: Expand Button (仅在需要时显示) */}
        {shouldShowExpandButton() && (
          <div
            className="flex items-center gap-1 cursor-pointer hover:text-foreground transition-colors px-2 py-1 rounded-md hover:bg-muted/50"
            onClick={e => {
              e.stopPropagation()
              void handleExpand() // Call async handler
            }}
          >
            {isLoadingDetail ? (
              <>
                <Loader2 size={12} className="animate-spin" />
                <span>{t('clipboard.item.loading')}</span>
              </>
            ) : (
              <>
                {isExpanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
                <span>
                  {isExpanded ? t('clipboard.item.collapse') : t('clipboard.item.expand')}
                </span>
              </>
            )}
          </div>
        )}

        {/* Right: Stats & Index */}
        <div className="flex items-center gap-4 min-w-20 justify-end">
          <span>{getSizeInfo()}</span>
          <span className="font-mono text-muted-foreground/40">{index}</span>
        </div>
      </div>
    </div>
  )
}

export default ClipboardItem
