import { openUrl } from '@tauri-apps/plugin-opener'
import {
  ChevronDown,
  ChevronUp,
  File,
  ExternalLink,
  Image as ImageIcon,
  Loader2,
} from 'lucide-react'
import React, { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ClipboardTextItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardCodeItem,
  ClipboardFileItem,
  fetchClipboardResourceText,
  resolveResourceImageUrl,
} from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { formatFileSize } from '@/utils'
import VirtualizedText from './VirtualizedText'

const log = createLogger('clipboard-item')

/** Threshold above which we switch to chunked rendering for performance. */
const LARGE_TEXT_THRESHOLD = 50_000

/** Target characters per group for content-visibility optimization. */
const GROUP_TARGET_SIZE = 5000

/**
 * Renders large text with performance optimization.
 * - Multi-line text: groups lines into block divs with content-visibility: auto
 * - Single-line huge text (no/few newlines): falls back to react-virtuoso
 *   with a fixed-height container since it's impossible to render 500KB+
 *   in one DOM node without freezing
 */
const ChunkedText: React.FC<{ text: string }> = ({ text }) => {
  const lines = useMemo(() => text.split('\n'), [text])
  const hasLongLine = useMemo(() => lines.some(line => line.length > LARGE_TEXT_THRESHOLD), [lines])
  const groups = useMemo(() => {
    if (hasLongLine) return []
    const result: string[] = []
    let current: string[] = []
    let currentSize = 0
    for (const line of lines) {
      current.push(line)
      currentSize += line.length
      if (currentSize >= GROUP_TARGET_SIZE) {
        result.push(current.join('\n'))
        current = []
        currentSize = 0
      }
    }
    if (current.length > 0) {
      result.push(current.join('\n'))
    }
    return result
  }, [lines, hasLongLine])

  // Single-line or few-line huge text: use virtualized rendering
  if (hasLongLine) {
    return <VirtualizedText text={text} className="h-96" />
  }

  return (
    <div>
      {groups.map((group, i) => (
        <div
          key={`chunk-${i}-${group.length}`}
          className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90"
          style={{
            wordBreak: 'break-all',
            contentVisibility: 'auto',
            containIntrinsicSize: 'auto 3em',
          }}
        >
          {group}
        </div>
      ))}
    </div>
  )
}

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

interface ItemContentProps {
  type: ClipboardItemProps['type']
  content: ClipboardItemProps['content']
  isExpanded: boolean
  detailContent: string | null
  isLoadingDetail: boolean
  originalImageUrl: string | null
  isLoadingImage: boolean
  imageDimensions: { width: number; height: number } | null
  setImageDimensions: React.Dispatch<React.SetStateAction<{ width: number; height: number } | null>>
}

const ItemContent: React.FC<ItemContentProps> = ({
  type,
  content,
  isExpanded,
  detailContent,
  isLoadingDetail,
  originalImageUrl,
  isLoadingImage,
  imageDimensions,
  setImageDimensions,
}) => {
  const { t } = useTranslation()
  switch (type) {
    case 'text': {
      const textItem = content as ClipboardTextItem
      // Use detail content when expanded and available, otherwise use preview
      const textToShow = isExpanded && detailContent ? detailContent : textItem.display_text

      if (isLoadingDetail) {
        return (
          <p className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90 wrap-break-word">
            {t('clipboard.item.loading')}
          </p>
        )
      }

      // When expanded with large text, render in small block-level chunks
      // with content-visibility: auto so browser skips layout for off-screen chunks.
      if (isExpanded && textToShow.length > LARGE_TEXT_THRESHOLD) {
        return <ChunkedText text={textToShow} />
      }

      return (
        <p
          className={cn(
            'whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90 wrap-break-word',
            !isExpanded && 'line-clamp-5'
          )}
        >
          {textToShow}
        </p>
      )
    }
    case 'image': {
      const imageUrl = originalImageUrl
      const showLoadingState = isLoadingImage && !imageUrl
      return imageUrl ? (
        <img
          src={imageUrl}
          className={cn(
            'mx-auto w-auto object-contain rounded-md transition-all duration-300',
            isExpanded ? 'max-h-[32rem]' : 'max-h-32'
          )}
          alt={t('clipboard.item.altText.clipboardImage')}
          loading="lazy"
          onLoad={e => {
            const img = e.currentTarget
            if (!imageDimensions) {
              setImageDimensions({ width: img.naturalWidth, height: img.naturalHeight })
            }
          }}
        />
      ) : (
        <div className="flex flex-col items-center justify-center gap-2 h-32 w-full rounded-md bg-muted/30 border border-border/30">
          {showLoadingState ? (
            <Loader2 className="size-6 text-muted-foreground/70 animate-spin" />
          ) : (
            <ImageIcon className="size-6 text-muted-foreground/70" />
          )}
          <span className="text-xs text-muted-foreground/70">
            {showLoadingState ? t('clipboard.item.loading') : t('clipboard.preview.selectItem')}
          </span>
        </div>
      )
    }
    case 'link': {
      const linkItem = content as ClipboardLinkItem
      const firstUrl = linkItem.urls[0] ?? ''
      return (
        <div className="flex flex-col gap-1">
          <button
            type="button"
            className="text-left text-primary font-medium hover:underline break-all text-sm leading-relaxed flex items-center gap-2 cursor-pointer"
            onClick={e => {
              e.stopPropagation()
              openUrl(firstUrl).catch(err => log.error({ err }, 'Failed to open URL'))
            }}
          >
            <ExternalLink size={14} />
            {firstUrl}
          </button>
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
            <div
              key={`${name}-${i}`}
              className="flex items-center gap-2 text-sm text-foreground/80"
            >
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

/**
 * Inner body holding all entry-scoped state (expand state, fetched detail
 * content, fetched original image URL/dimensions). It is rendered with
 * `key={entryId}` from the parent so that when entryId changes React
 * remounts this subtree and re-creates the state from scratch — replacing
 * the previous useEffect that manually cleared image state on entryId/type
 * change (no-derived-state-effect).
 */
interface ClipboardItemBodyProps {
  index: number
  type: ClipboardItemProps['type']
  time: string
  content: ClipboardItemProps['content']
  entryId: string
  fileSize?: number
}

const ClipboardItemBody: React.FC<ClipboardItemBodyProps> = ({
  index,
  type,
  time,
  content,
  entryId,
  fileSize,
}) => {
  const { t } = useTranslation()
  const [isExpanded, setIsExpanded] = useState(false)
  const [detailContent, setDetailContent] = useState<string | null>(null)
  const [originalImageUrl, setOriginalImageUrl] = useState<string | null>(null)
  const [isLoadingDetail, setIsLoadingDetail] = useState(false)
  const [isLoadingImage, setIsLoadingImage] = useState(false)
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(
    null
  )

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
        if (!resource) {
          throw new Error('Resource not found')
        }
        const fullText = await fetchClipboardResourceText(resource)
        setDetailContent(fullText)
        setIsExpanded(true)
      } catch (e) {
        log.error({ err: e }, 'Failed to load detail')
        toast.error(t('clipboard.errors.loadDetailFailed'), {
          description: e instanceof Error ? e.message : t('clipboard.errors.unknown'),
        })
      } finally {
        setIsLoadingDetail(false)
      }
      return
    }

    if (type === 'image') {
      if (originalImageUrl) {
        setIsExpanded(true)
        return
      }

      setIsLoadingImage(true)
      try {
        const resource = await getClipboardEntryResource(entryId)
        const imageUrl = resource ? resolveResourceImageUrl(resource) : null
        setOriginalImageUrl(imageUrl)
        setIsExpanded(true)
      } catch (e) {
        log.error({ err: e }, 'Failed to load original image URL')
        toast.error(t('clipboard.errors.loadDetailFailed'), {
          description: e instanceof Error ? e.message : t('clipboard.errors.unknown'),
        })
      } finally {
        setIsLoadingImage(false)
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
      case 'image': {
        const imageItem = content as ClipboardImageItem
        const parts: string[] = []
        if (imageDimensions) {
          parts.push(`${imageDimensions.width}×${imageDimensions.height}`)
        }
        if (imageItem.size > 0) {
          parts.push(formatFileSize(imageItem.size))
        }
        return parts.length > 0 ? parts.join(' · ') : t('clipboard.item.image')
      }
      default:
        return ''
    }
  }

  return (
    <>
      {/* Main Content Area */}
      <div className="selectable select-text p-4">
        <ItemContent
          type={type}
          content={content}
          isExpanded={isExpanded}
          detailContent={detailContent}
          isLoadingDetail={isLoadingDetail}
          originalImageUrl={originalImageUrl}
          isLoadingImage={isLoadingImage}
          imageDimensions={imageDimensions}
          setImageDimensions={setImageDimensions}
        />
      </div>

      {/* Footer Area */}
      <div className="flex items-center justify-between px-4 pb-2 pt-1 text-xs text-muted-foreground/60 select-none">
        {/* Left: Time */}
        <div className="min-w-20">{time}</div>

        {/* Center: Expand Button (仅在需要时显示) */}
        {shouldShowExpandButton() && (
          <div
            role="button"
            tabIndex={0}
            className="flex items-center gap-1 hover:text-foreground transition-colors px-2 py-1 rounded-md hover:bg-muted/50"
            onClick={e => {
              e.stopPropagation()
              void handleExpand() // Call async handler
            }}
            onKeyDown={e => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                e.stopPropagation()
                void handleExpand()
              }
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
    </>
  )
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
  return (
    <div
      role="button"
      tabIndex={0}
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
      onKeyDown={e => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          const selection = window.getSelection()
          if (selection && selection.toString().length > 0) return
          onSelect?.(e as unknown as React.MouseEvent<HTMLDivElement>)
        }
      }}
    >
      {/*
        key={entryId} ensures the body subtree is remounted whenever the
        entry identity changes, so image-related state (URL, loading flag,
        dimensions) is reset for free — replacing the previous in-body
        useEffect that did this manually (no-derived-state-effect).
      */}
      <ClipboardItemBody
        key={entryId}
        index={index}
        type={type}
        time={time}
        content={content}
        entryId={entryId}
        fileSize={fileSize}
      />
    </div>
  )
}

export default ClipboardItem
